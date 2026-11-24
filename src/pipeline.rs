use crate::{
    BoundingBox, EvidenceBlock, OcrEngine, OcrBlock, PerceptionError, PerceptionRequest,
    PerceptionResult,
};
use base64::Engine as _;
use image::{DynamicImage, GenericImageView, ImageFormat, imageops};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct PerceptionConfig {
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub minimum_confidence: f32,
    pub line_overlap_ratio: f32,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            max_width: 12_000,
            max_height: 12_000,
            max_pixels: 80_000_000,
            minimum_confidence: 0.72,
            line_overlap_ratio: 0.35,
        }
    }
}

pub struct PerceptionEngine {
    ocr: Arc<dyn OcrEngine>,
    config: PerceptionConfig,
}

impl PerceptionEngine {
    pub fn new(ocr: Arc<dyn OcrEngine>, config: PerceptionConfig) -> Self {
        Self { ocr, config }
    }

    pub fn process(&self, request: PerceptionRequest) -> Result<PerceptionResult, PerceptionError> {
        let encoded = base64::engine::general_purpose::STANDARD
            .decode(request.image_base64.as_bytes())
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(request.image_base64.as_bytes()))
            .map_err(|error| PerceptionError::Ocr(format!("invalid base64 image: {error}")))?;
        let format = image::guess_format(&encoded)?;
        if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP) {
            return Err(PerceptionError::Ocr("only JPEG, PNG and WebP are accepted".into()));
        }
        let original = image::load_from_memory(&encoded)?;
        let (original_width, original_height) = original.dimensions();
        if original_width > self.config.max_width
            || original_height > self.config.max_height
            || (original_width as u64) * (original_height as u64) > self.config.max_pixels
        {
            return Err(PerceptionError::ImageTooLarge);
        }

        let rotation = request.rotation_degrees % 360;
        if rotation % 90 != 0 {
            return Err(PerceptionError::Ocr("rotationDegrees must be a multiple of 90".into()));
        }
        let rgba = orient(original, rotation).to_rgba8();
        let (ocr_image, bounds_to_oriented) = match request.region {
            Some(region) => {
                if !region.valid_for(rgba.width(), rgba.height()) {
                    return Err(PerceptionError::Ocr("region lies outside the oriented image".into()));
                }
                let left = region.left.floor() as u32;
                let top = region.top.floor() as u32;
                let width = region.right.ceil() as u32 - left;
                let height = region.bottom.ceil() as u32 - top;
                (imageops::crop_imm(&rgba, left, top, width, height).to_image(), (left as f32, top as f32))
            }
            None => (rgba.clone(), (0.0, 0.0)),
        };

        let mut raw = self.ocr.recognize(&ocr_image, &request.language)
            .map_err(PerceptionError::Ocr)?;
        raw.retain(|block| !block.text.trim().is_empty());
        if raw.is_empty() { return Err(PerceptionError::EmptyRecognition); }
        for block in &raw {
            if !block.bounds.valid_for(ocr_image.width(), ocr_image.height())
                || !block.confidence.is_finite() || !(0.0..=1.0).contains(&block.confidence)
            {
                return Err(PerceptionError::Ocr("adapter returned invalid bounds or confidence".into()));
            }
        }

        let (ow, oh) = (original_width, original_height);
        let mut warnings = Vec::new();
        let mut blocks = reading_order(raw, self.config.line_overlap_ratio)
            .into_iter().enumerate().map(|(index, mut block)| {
                block.bounds.left += bounds_to_oriented.0;
                block.bounds.right += bounds_to_oriented.0;
                block.bounds.top += bounds_to_oriented.1;
                block.bounds.bottom += bounds_to_oriented.1;
                block.bounds = oriented_to_original(block.bounds, ow, oh, rotation);
                EvidenceBlock {
                    evidence_id: format!("{}:e{:03}", request.photo_id, index + 1),
                    text: block.text.trim().to_owned(),
                    kind: block.kind,
                    confidence: block.confidence,
                    original_bounds: block.bounds,
                    reading_order: index + 1,
                    needs_review: block.confidence < self.config.minimum_confidence,
                }
            }).collect::<Vec<_>>();

        if blocks.iter().any(|block| block.needs_review) {
            warnings.push("Some OCR blocks are below the review-confidence threshold.".into());
        }
        if rotation != 0 {
            warnings.push("Bounds were mapped back to the original photo after right-angle rotation.".into());
        }
        let reading_order = blocks.iter().map(|block| block.evidence_id.clone()).collect();
        // Keep the final output order as the same deterministic reading order.
        blocks.shrink_to_fit();
        Ok(PerceptionResult {
            schema_version: 1,
            photo_id: request.photo_id,
            original_width: ow,
            original_height: oh,
            reading_order,
            blocks,
            warnings,
        })
    }
}

fn orient(image: DynamicImage, degrees: u16) -> DynamicImage {
    match degrees {
        90 => image.rotate90(),
        180 => image.rotate180(),
        270 => image.rotate270(),
        _ => image,
    }
}

/// Transform a box in the clockwise-rotated image back to original pixels.
fn oriented_to_original(b: BoundingBox, width: u32, height: u32, clockwise: u16) -> BoundingBox {
    let w = width as f32;
    let h = height as f32;
    match clockwise {
        90 => BoundingBox { left: b.top, top: h - b.right, right: b.bottom, bottom: h - b.left },
        180 => BoundingBox { left: w - b.right, top: h - b.bottom, right: w - b.left, bottom: h - b.top },
        270 => BoundingBox { left: w - b.bottom, top: b.left, right: w - b.top, bottom: b.right },
        _ => b,
    }
}

/// Stable layout-aware ordering: form horizontal bands, then order each band
/// left-to-right. RTL is deliberately not inferred; extend via an explicit locale policy.
pub fn reading_order(mut blocks: Vec<OcrBlock>, overlap_ratio: f32) -> Vec<OcrBlock> {
    blocks.sort_by(|a, b| a.bounds.center_y().total_cmp(&b.bounds.center_y())
        .then_with(|| a.bounds.left.total_cmp(&b.bounds.left)));
    let mut bands: Vec<Vec<OcrBlock>> = Vec::new();
    for block in blocks {
        let joins = bands.last().and_then(|band| band.last()).is_some_and(|last| {
            let overlap = last.bounds.bottom.min(block.bounds.bottom) - last.bounds.top.max(block.bounds.top);
            let min_height = (last.bounds.bottom-last.bounds.top).min(block.bounds.bottom-block.bounds.top);
            min_height > 0.0 && overlap / min_height >= overlap_ratio
        });
        if joins { bands.last_mut().expect("band exists").push(block); }
        else { bands.push(vec![block]); }
    }
    for band in &mut bands {
        band.sort_by(|a,b| a.bounds.left.total_cmp(&b.bounds.left));
    }
    bands.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockKind, OcrBlock};
    use image::{ImageBuffer, Rgba, RgbaImage};
    use std::io::Cursor;
    use std::sync::Arc;

    struct FakeOcr(Vec<OcrBlock>);
    impl OcrEngine for FakeOcr {
        fn recognize(&self, _: &RgbaImage, _: &str) -> Result<Vec<OcrBlock>, String> { Ok(self.0.clone()) }
    }

    fn block(text: &str, bounds: BoundingBox, confidence: f32) -> OcrBlock {
        OcrBlock { text: text.into(), bounds, confidence, kind: BlockKind::Text }
    }

    fn request(rotation: u16) -> PerceptionRequest {
        let image = ImageBuffer::from_pixel(100, 80, Rgba([255,255,255,255]));
        let mut bytes=Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut bytes,ImageFormat::Png).unwrap();
        PerceptionRequest { photo_id:"P1".into(), image_base64:base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()), rotation_degrees:rotation, language:"zh-CN".into(), region:None }
    }

    #[test]
    fn orders_rows_top_to_bottom_and_columns_left_to_right() {
        let input=vec![block("右上",BoundingBox{left:50.0,top:0.0,right:70.0,bottom:10.0},0.9),block("左下",BoundingBox{left:0.0,top:20.0,right:20.0,bottom:30.0},0.9),block("左上",BoundingBox{left:0.0,top:1.0,right:20.0,bottom:11.0},0.9)];
        let output=reading_order(input,0.35).into_iter().map(|b|b.text).collect::<Vec<_>>();
        assert_eq!(output,["左上","右上","左下"]);
    }

    #[test]
    fn rotation_maps_bounds_back_to_original_photo() {
        let engine=PerceptionEngine::new(Arc::new(FakeOcr(vec![block("公式",BoundingBox{left:10.0,top:20.0,right:30.0,bottom:40.0},0.95)])),PerceptionConfig::default());
        let result=engine.process(request(90)).unwrap();
        assert_eq!((result.original_width,result.original_height),(100,80));
        assert_eq!(result.blocks[0].original_bounds,BoundingBox{left:20.0,top:50.0,right:40.0,bottom:70.0});
    }

    #[test]
    fn low_confidence_is_returned_not_guessed() {
        let engine=PerceptionEngine::new(Arc::new(FakeOcr(vec![block("公式？",BoundingBox{left:0.0,top:0.0,right:30.0,bottom:10.0},0.4)])),PerceptionConfig::default());
        let result=engine.process(request(0)).unwrap();
        assert!(result.blocks[0].needs_review);
        assert_eq!(result.blocks[0].text,"公式？");
        assert!(!result.warnings.is_empty());
    }
}