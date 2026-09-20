use std::{io::Cursor, time::Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, RgbaImage};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

use crate::desktop::Frame;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox {
    pub center_x: f64,
    pub center_y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug)]
pub struct GroundingResult {
    pub bbox: BoundingBox,
    pub raw_response: String,
    pub frame_acquire_ms: f64,
    pub png_encode_ms: f64,
    pub base64_ms: f64,
    pub vocaela_http_ms: f64,
    pub vocaela_parse_ms: f64,
    pub grounding_total_ms: f64,
    pub png: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("Vocaela is unavailable at {0}: {1}")]
    Unavailable(String, String),
    #[error("Vocaela HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Vocaela returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("invalid normalized bounding box: {0}")]
    InvalidBoundingBox(String),
    #[error("image encoding failed: {0}")]
    Image(#[from] image::ImageError),
}

#[derive(Clone)]
pub struct VocaelaClient {
    client: Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}
#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

impl VocaelaClient {
    pub fn new(client: Client, base_url: String) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').into(),
        }
    }

    pub async fn health_check(&self) -> Result<(), VisionError> {
        let health = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await;
        if matches!(health, Ok(response) if response.status().is_success()) {
            return Ok(());
        }
        let models = self
            .client
            .get(format!("{}/v1/models", self.base_url))
            .send()
            .await;
        if matches!(models, Ok(ref response) if response.status().is_success()) {
            return Ok(());
        }
        let detail = match models {
            Ok(response) => response.status().to_string(),
            Err(error) => error.to_string(),
        };
        Err(VisionError::Unavailable(self.base_url.clone(), detail))
    }

    pub async fn ground(
        &self,
        frame: &Frame,
        target: &str,
    ) -> Result<GroundingResult, VisionError> {
        let total = Instant::now();
        let png_started = Instant::now();
        let image = RgbaImage::from_raw(frame.pixel_width, frame.pixel_height, frame.rgba.clone())
            .ok_or_else(|| {
                VisionError::InvalidResponse("frame RGBA buffer has the wrong size".into())
            })?;
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut png, ImageFormat::Png)?;
        let png = png.into_inner();
        let png_encode_ms = png_started.elapsed().as_secs_f64() * 1000.0;
        let base_started = Instant::now();
        let encoded = STANDARD.encode(&png);
        let base64_ms = base_started.elapsed().as_secs_f64() * 1000.0;
        let request = serde_json::json!({
            "model": "vocaela",
            "messages": [{"role":"user", "content":[
                {"type":"text", "text": format!("Click {}", target)},
                {"type":"image_url", "image_url":{"url": format!("data:image/png;base64,{}", encoded)}}
            ]}], "temperature": 0, "max_tokens": 64
        });
        let http_started = Instant::now();
        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()?;
        let body: ChatResponse = response
            .json()
            .await
            .map_err(|e| VisionError::InvalidResponse(e.to_string()))?;
        let vocaela_http_ms = http_started.elapsed().as_secs_f64() * 1000.0;
        let parse_started = Instant::now();
        let raw_response = body
            .choices
            .first()
            .ok_or_else(|| VisionError::InvalidResponse("choices is empty".into()))?
            .message
            .content
            .clone();
        tracing::debug!(target = target, response = %raw_response, "Vocaela response");
        let bbox = parse_bbox(&raw_response)?;
        let vocaela_parse_ms = parse_started.elapsed().as_secs_f64() * 1000.0;
        Ok(GroundingResult {
            bbox,
            raw_response,
            frame_acquire_ms: 0.0,
            png_encode_ms,
            base64_ms,
            vocaela_http_ms,
            vocaela_parse_ms,
            grounding_total_ms: total.elapsed().as_secs_f64() * 1000.0,
            png,
        })
    }
}

pub fn parse_bbox(text: &str) -> Result<BoundingBox, VisionError> {
    let regex = Regex::new(r"\[\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+))\s*,\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+))\s*,\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+))\s*,\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+))\s*\]").expect("literal bbox regex");
    let values = if let Some(captures) = regex.captures(text) {
        captures
            .iter()
            .skip(1)
            .map(|capture| capture.unwrap().as_str().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                VisionError::InvalidBoundingBox("numbers are not finite decimals".into())
            })?
    } else {
        let json_text = text
            .trim()
            .strip_prefix("```json")
            .and_then(|value| value.strip_suffix("```"))
            .or_else(|| {
                text.trim()
                    .strip_prefix("```")
                    .and_then(|value| value.strip_suffix("```"))
            })
            .unwrap_or(text.trim());
        let parsed: serde_json::Value = serde_json::from_str(json_text.trim()).map_err(|_| {
            VisionError::InvalidBoundingBox(
                "expected four numbers in brackets or coordinate JSON".into(),
            )
        })?;
        let mut found = None;
        find_json_coordinates(&parsed, &mut found);
        found.ok_or_else(|| {
            VisionError::InvalidBoundingBox("JSON did not contain normalized coordinates".into())
        })?
    };
    let bbox = BoundingBox {
        center_x: values[0],
        center_y: values[1],
        width: values[2],
        height: values[3],
    };
    if !bbox.center_x.is_finite()
        || !bbox.center_y.is_finite()
        || !bbox.width.is_finite()
        || !bbox.height.is_finite()
        || !(0.0..=1.0).contains(&bbox.center_x)
        || !(0.0..=1.0).contains(&bbox.center_y)
        || !(0.0..=1.0).contains(&bbox.width)
        || !(0.0..=1.0).contains(&bbox.height)
        || bbox.width == 0.0
        || bbox.height == 0.0
    {
        return Err(VisionError::InvalidBoundingBox(format!(
            "values out of range: {bbox:?}"
        )));
    }
    Ok(bbox)
}

fn normalized_number(value: &serde_json::Value) -> Option<f64> {
    let value = value.as_f64()?;
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

fn find_json_coordinates(value: &serde_json::Value, result: &mut Option<Vec<f64>>) {
    if result.is_some() {
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            let x = object
                .get("center_x")
                .or_else(|| object.get("x_normalized"))
                .or_else(|| object.get("x"))
                .and_then(normalized_number);
            let y = object
                .get("center_y")
                .or_else(|| object.get("y_normalized"))
                .or_else(|| object.get("y"))
                .and_then(normalized_number);
            let width = object
                .get("width")
                .and_then(normalized_number)
                .unwrap_or(0.01);
            let height = object
                .get("height")
                .and_then(normalized_number)
                .unwrap_or(0.01);
            if let (Some(x), Some(y)) = (x, y) {
                *result = Some(vec![x, y, width, height]);
                return;
            }
            for key in [
                "coordinates",
                "coordinate",
                "point",
                "center",
                "bbox",
                "bounding_box",
                "action",
            ] {
                if let Some(nested) = object.get(key) {
                    find_json_coordinates(nested, result);
                    if result.is_some() {
                        return;
                    }
                }
            }
            for nested in object.values() {
                find_json_coordinates(nested, result);
                if result.is_some() {
                    return;
                }
            }
        }
        serde_json::Value::Array(array) => {
            if array.len() >= 4 {
                if let Some(values) = array[..4]
                    .iter()
                    .map(normalized_number)
                    .collect::<Option<Vec<_>>>()
                {
                    *result = Some(values);
                    return;
                }
            }
            if array.len() >= 2 {
                if let (Some(x), Some(y)) =
                    (normalized_number(&array[0]), normalized_number(&array[1]))
                {
                    *result = Some(vec![x, y, 0.01, 0.01]);
                    return;
                }
            }
            for nested in array {
                find_json_coordinates(nested, result);
                if result.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bbox() {
        let bbox = parse_bbox("Normalized bounding box: [0.3849, 0.0518, 0.0547, 0.0266]").unwrap();
        assert_eq!(bbox.center_x, 0.3849);
    }
    #[test]
    fn rejects_invalid_bbox() {
        assert!(parse_bbox("[1.4, -0.1, 0.1, 0.1]").is_err());
        assert!(parse_bbox("[NaN, 0.1, 0.1, 0.1]").is_err());
    }

    #[test]
    fn parses_coordinate_json() {
        let bbox = parse_bbox(r#"{"action":{"coordinates":{"x":0.25,"y":0.75}}}"#).unwrap();
        assert_eq!((bbox.center_x, bbox.center_y), (0.25, 0.75));
    }
}
