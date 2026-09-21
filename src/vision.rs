use std::{io::Cursor, time::Instant};

use async_trait::async_trait;
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
pub struct FoundGrounding {
    pub bbox: BoundingBox,
    pub raw_response: String,
    pub frame_acquire_ms: f64,
    pub image_encode_ms: f64,
    pub base64_ms: f64,
    pub vocaela_http_ms: f64,
    pub vision_parse_ms: f64,
    pub grounding_total_ms: f64,
    pub png: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum GroundingResult {
    Found(FoundGrounding),
    NotFound { raw_response: String },
}

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("vision provider is unavailable at {0}: {1}")]
    Unavailable(String, String),
    #[error("vision HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("vision response is invalid: {0}")]
    InvalidResponse(String),
    #[error("invalid normalized bounding box: {0}")]
    InvalidBoundingBox(String),
    #[error("image encoding failed: {0}")]
    Image(#[from] image::ImageError),
}

#[async_trait]
pub trait VisionProvider: Send + Sync {
    async fn health_check(&self) -> Result<(), VisionError>;
    async fn ground(
        &self,
        frame: &Frame,
        instruction: &str,
    ) -> Result<GroundingResult, VisionError>;
}

#[derive(Clone)]
pub struct VocaelaVisionProvider {
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

impl VocaelaVisionProvider {
    pub fn new(client: Client, base_url: String) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').into(),
        }
    }
}

#[async_trait]
impl VisionProvider for VocaelaVisionProvider {
    async fn health_check(&self) -> Result<(), VisionError> {
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

    async fn ground(
        &self,
        frame: &Frame,
        instruction: &str,
    ) -> Result<GroundingResult, VisionError> {
        let total = Instant::now();
        let encode_started = Instant::now();
        let image = RgbaImage::from_raw(frame.pixel_width, frame.pixel_height, frame.rgba.clone())
            .ok_or_else(|| {
                VisionError::InvalidResponse("frame RGBA buffer has the wrong size".into())
            })?;
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut png, ImageFormat::Png)?;
        let png = png.into_inner();
        let image_encode_ms = encode_started.elapsed().as_secs_f64() * 1000.0;
        let base_started = Instant::now();
        let encoded = STANDARD.encode(&png);
        let base64_ms = base_started.elapsed().as_secs_f64() * 1000.0;
        let request = serde_json::json!({ "model": "vocaela", "messages": [{"role":"user", "content":[{"type":"text", "text": instruction}, {"type":"image_url", "image_url":{"url": format!("data:image/png;base64,{encoded}")}}]}], "temperature": 0, "max_tokens": 64 });
        let http_started = Instant::now();
        let body: ChatResponse = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(|error| VisionError::InvalidResponse(error.to_string()))?;
        let vocaela_http_ms = http_started.elapsed().as_secs_f64() * 1000.0;
        let parse_started = Instant::now();
        let raw_response = body
            .choices
            .first()
            .ok_or_else(|| VisionError::InvalidResponse("choices is empty".into()))?
            .message
            .content
            .clone();
        let lower = raw_response.to_ascii_lowercase();
        if ["not found", "cannot find", "can't find", "no target"]
            .iter()
            .any(|phrase| lower.contains(phrase))
        {
            return Ok(GroundingResult::NotFound { raw_response });
        }
        let bbox = parse_bbox(&raw_response).map_err(|error| match error {
            VisionError::InvalidBoundingBox(detail) => VisionError::InvalidBoundingBox(format!(
                "{detail}; raw response: {:?}",
                raw_response.chars().take(500).collect::<String>()
            )),
            error => error,
        })?;
        Ok(GroundingResult::Found(FoundGrounding {
            bbox,
            raw_response,
            frame_acquire_ms: 0.0,
            image_encode_ms,
            base64_ms,
            vocaela_http_ms,
            vision_parse_ms: parse_started.elapsed().as_secs_f64() * 1000.0,
            grounding_total_ms: total.elapsed().as_secs_f64() * 1000.0,
            png,
        }))
    }
}

pub fn parse_bbox(text: &str) -> Result<BoundingBox, VisionError> {
    let number = r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)";
    let regex = Regex::new(&format!(
        r"\[\s*({number})\s*,\s*({number})\s*,\s*({number})\s*,\s*({number})\s*\]"
    ))
    .expect("literal bbox regex");
    let values = if let Some(captures) = regex.captures(text) {
        captures
            .iter()
            .skip(1)
            .map(|capture| capture.unwrap().as_str().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| VisionError::InvalidBoundingBox("numbers are not decimals".into()))?
    } else if let Some(captures) =
        Regex::new(&format!(r"[\[(]\s*({number})\s*,\s*({number})\s*[\])]"))
            .expect("literal coordinate regex")
            .captures(text)
    {
        let x = captures[1]
            .parse::<f64>()
            .map_err(|_| VisionError::InvalidBoundingBox("point x is not a decimal".into()))?;
        let y = captures[2]
            .parse::<f64>()
            .map_err(|_| VisionError::InvalidBoundingBox("point y is not a decimal".into()))?;
        // A two-value response is an explicit center point. Normalize it to a
        // small non-zero box so the common BoundingBox path remains uniform
        // for debug and input conversion.
        vec![x, y, 0.01, 0.01]
    } else {
        let json = text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let value: serde_json::Value = serde_json::from_str(json).map_err(|_| {
            VisionError::InvalidBoundingBox("expected a normalized box or coordinate JSON".into())
        })?;
        find_json_coordinates(&value).ok_or_else(|| {
            VisionError::InvalidBoundingBox("JSON lacks normalized coordinates".into())
        })?
    };
    let bbox = BoundingBox {
        center_x: values[0],
        center_y: values[1],
        width: values[2],
        height: values[3],
    };
    if ![bbox.center_x, bbox.center_y, bbox.width, bbox.height]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        || bbox.width == 0.0
        || bbox.height == 0.0
    {
        return Err(VisionError::InvalidBoundingBox(format!(
            "values out of range: {bbox:?}"
        )));
    }
    Ok(bbox)
}

fn number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
}
fn find_json_coordinates(value: &serde_json::Value) -> Option<Vec<f64>> {
    match value {
        serde_json::Value::Object(object) => {
            let x = object
                .get("center_x")
                .or_else(|| object.get("x_normalized"))
                .or_else(|| object.get("x"))
                .and_then(number);
            let y = object
                .get("center_y")
                .or_else(|| object.get("y_normalized"))
                .or_else(|| object.get("y"))
                .and_then(number);
            if let (Some(x), Some(y)) = (x, y) {
                return Some(vec![
                    x,
                    y,
                    object.get("width").and_then(number).unwrap_or(0.01),
                    object.get("height").and_then(number).unwrap_or(0.01),
                ]);
            }
            object.values().find_map(find_json_coordinates)
        }
        serde_json::Value::Array(array) if array.len() >= 4 => {
            array[..4].iter().map(number).collect()
        }
        serde_json::Value::Array(array) if array.len() >= 2 => {
            Some(vec![number(&array[0])?, number(&array[1])?, 0.01, 0.01])
        }
        serde_json::Value::Array(array) => array.iter().find_map(find_json_coordinates),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bbox() {
        assert_eq!(
            parse_bbox("[0.3849, 0.0518, 0.0547, 0.0266]")
                .unwrap()
                .center_x,
            0.3849
        );
    }
    #[test]
    fn rejects_invalid_bbox() {
        assert!(parse_bbox("[1.4, -0.1, 0.1, 0.1]").is_err());
    }
    #[test]
    fn parses_coordinate_json() {
        assert_eq!(
            parse_bbox(r#"{"action":{"coordinates":{"x":0.25,"y":0.75}}}"#)
                .unwrap()
                .center_y,
            0.75
        );
    }
    #[test]
    fn parses_normalized_center_point() {
        let bbox = parse_bbox("Normalized point: (0.25, 0.75)").unwrap();
        assert_eq!((bbox.center_x, bbox.center_y), (0.25, 0.75));
        assert_eq!((bbox.width, bbox.height), (0.01, 0.01));
    }
}
