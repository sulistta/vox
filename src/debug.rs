use std::{
    fs,
    path::{Path, PathBuf},
};

use image::{Rgba, RgbaImage};
use serde::Serialize;

use crate::{
    action::{ActionRecord, AgentState},
    desktop::Frame,
    jev::DecisionCandidates,
    vision::BoundingBox,
};

#[derive(Clone)]
pub struct DebugWriter {
    root: PathBuf,
    persist_screenshots: bool,
}

impl DebugWriter {
    pub fn new(root: impl AsRef<Path>, persist_screenshots: bool) -> std::io::Result<Self> {
        let root = root.as_ref().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            persist_screenshots,
        })
    }

    pub fn write_step(
        &self,
        step: usize,
        frame: &Frame,
        bbox: Option<BoundingBox>,
        target: &str,
        records: &[ActionRecord],
        png: Option<&[u8]>,
    ) -> std::io::Result<()> {
        if self.persist_screenshots {
            let original = self.root.join(format!("step-{step:02}-frame.png"));
            if let Some(png) = png {
                fs::write(&original, png)?;
            } else {
                let image =
                    RgbaImage::from_raw(frame.pixel_width, frame.pixel_height, frame.rgba.clone())
                        .ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "invalid RGBA frame",
                            )
                        })?;
                image.save(&original).map_err(std::io::Error::other)?;
            }
            let mut overlay =
                RgbaImage::from_raw(frame.pixel_width, frame.pixel_height, frame.rgba.clone())
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid RGBA frame")
                    })?;
            if let Some(bbox) = bbox {
                let (x, y) = frame.normalized_to_pixel(bbox);
                draw_circle(
                    &mut overlay,
                    x as i32,
                    y as i32,
                    14,
                    Rgba([255, 40, 40, 255]),
                );
                draw_cross(&mut overlay, x as i32, y as i32, Rgba([255, 255, 0, 255]));
            }
            overlay
                .save(self.root.join(format!("step-{step:02}-overlay.png")))
                .map_err(std::io::Error::other)?;
        }
        #[derive(Serialize)]
        struct Step<'a> {
            step: usize,
            target: &'a str,
            bbox: Option<BoundingBoxJson>,
            records: &'a [ActionRecord],
        }
        #[derive(Serialize)]
        struct BoundingBoxJson {
            center_x: f64,
            center_y: f64,
            width: f64,
            height: f64,
        }
        let bbox = bbox.map(|b| BoundingBoxJson {
            center_x: b.center_x,
            center_y: b.center_y,
            width: b.width,
            height: b.height,
        });
        let json = serde_json::to_vec_pretty(&Step {
            step,
            target,
            bbox,
            records,
        })
        .map_err(std::io::Error::other)?;
        fs::write(self.root.join(format!("step-{step:02}.json")), json)
    }

    pub fn write_decision(
        &self,
        step: usize,
        state: &AgentState,
        selected_capability: &str,
        confidence: f64,
        probabilities: &std::collections::BTreeMap<String, f64>,
        candidates: &DecisionCandidates,
    ) -> std::io::Result<()> {
        #[derive(Serialize)]
        struct Decision<'a> {
            step: usize,
            selected_capability: &'a str,
            confidence: f64,
            probabilities: &'a std::collections::BTreeMap<String, f64>,
            candidates: &'a DecisionCandidates,
            state: &'a AgentState,
        }
        let json = serde_json::to_vec_pretty(&Decision {
            step,
            selected_capability,
            confidence,
            probabilities,
            candidates,
            state,
        })
        .map_err(std::io::Error::other)?;
        fs::write(
            self.root.join(format!("step-{step:02}-decision.json")),
            json,
        )
    }

    pub fn write_metrics(&self, step: usize, records: &[ActionRecord]) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(&records.last()).map_err(std::io::Error::other)?;
        fs::write(self.root.join(format!("step-{step:02}-metrics.json")), json)
    }
}

fn draw_circle(image: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let distance = dx * dx + dy * dy;
            if distance >= (radius - 1) * (radius - 1) && distance <= (radius + 1) * (radius + 1) {
                let x = cx + dx;
                let y = cy + dy;
                if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
                    image.put_pixel(x as u32, y as u32, color);
                }
            }
        }
    }
}

fn draw_cross(image: &mut RgbaImage, cx: i32, cy: i32, color: Rgba<u8>) {
    for delta in -8..=8 {
        for (x, y) in [(cx + delta, cy), (cx, cy + delta)] {
            if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
                image.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}
