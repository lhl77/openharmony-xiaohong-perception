//! Feed detections from an external OCR engine into the real M1 Rust pipeline.
//!
//! Usage: cargo run --example rapidocr_fixture -- photo.jpg detections.json
//! detections.json is an array of {"text","confidence","corners":[[x,y],...]}
//! in the original photo's pixel coordinate system.

use base64::Engine as _;
use serde::Deserialize;
use std::{env, fs, sync::Arc};
use xiaohong_perception::{
    BlockKind, BoundingBox, OcrBlock, OcrEngine, PerceptionConfig, PerceptionEngine,
    PerceptionRequest,
};

#[derive(Deserialize)]
struct Detection {
    text: String,
    confidence: f32,
    corners: Vec<[f32; 2]>,
}

struct FixtureEngine(Vec<OcrBlock>);

impl OcrEngine for FixtureEngine {
    fn recognize(&self, _image: &image::RgbaImage, _language: &str) -> Result<Vec<OcrBlock>, String> {
        Ok(self.0.clone())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let photo_path = args.next().ok_or("missing photo path")?;
    let detections_path = args.next().ok_or("missing OCR detections JSON")?;
    let photo = fs::read(&photo_path)?;
    let raw: Vec<Detection> = serde_json::from_slice(&fs::read(detections_path)?)?;
    let blocks = raw
        .into_iter()
        .map(|item| {
            if item.corners.len() != 4 {
                return Err("OCR polygon must have four corners".into());
            }
            let bounds = BoundingBox {
                left: item.corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                top: item.corners.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                right: item.corners.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
                bottom: item.corners.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
            };
            Ok(OcrBlock {
                text: item.text,
                bounds,
                // Encode review status for the current OcrBlock contract: the
                // pipeline flags confidence below its 0.72 threshold.
                confidence: item.confidence,
                kind: BlockKind::Text,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;

    let request = PerceptionRequest {
        photo_id: "M1-demo-board-2026-11-24".into(),
        image_base64: base64::engine::general_purpose::STANDARD.encode(photo),
        rotation_degrees: 0,
        language: "zh-CN".into(),
        region: None,
    };
    let engine = PerceptionEngine::new(Arc::new(FixtureEngine(blocks)), PerceptionConfig::default());
    let result = engine.process(request)?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "ocrAdapter": "RapidOCR",
        "note": "OCR detections are from RapidOCR; EvidenceBlock IDs/order/schema are produced by the Rust M1 pipeline.",
        "result": result,
    }))?);
    Ok(())
}