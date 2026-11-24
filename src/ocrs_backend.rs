//! Optional OCR backend backed by `robertknight/ocrs` + `rten`.
//!
//! Enabled with `--features ocrs-backend`. The bundled ocrs models recognize
//! horizontally laid out printed text using a Latin alphabet; CJK and printed
//! math require a custom recognition model plus a matching `alphabet`.

use crate::types::{InitRequest, OcrEngine, PerceptionError};

#[cfg(feature = "ocrs-backend")]
use crate::types::{BlockKind, BoundingBox, OcrBlock};
#[cfg(feature = "ocrs-backend")]
use image::RgbaImage;
#[cfg(feature = "ocrs-backend")]
use ocrs::TextItem;

#[cfg(feature = "ocrs-backend")]
pub struct OcrsEngine {
    inner: ocrs::OcrEngine,
}

#[cfg(feature = "ocrs-backend")]
impl OcrEngine for OcrsEngine {
    fn recognize(&self, image: &RgbaImage, _language: &str) -> Result<Vec<OcrBlock>, String> {
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
        let source = ocrs::ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions())
            .map_err(|error| error.to_string())?;
        let input = self.inner.prepare_input(source).map_err(|error| error.to_string())?;
        let words = self.inner.detect_words(&input).map_err(|error| error.to_string())?;
        let lines = self.inner.find_text_lines(&input, &words);
        let recognized = self
            .inner
            .recognize_text(&input, &lines)
            .map_err(|error| error.to_string())?;

        let mut blocks = Vec::new();
        for line in recognized.iter().flatten() {
            let text = line.to_string();
            if text.trim().is_empty() {
                continue;
            }
            let rect = line.bounding_rect();
            blocks.push(OcrBlock {
                text,
                bounds: BoundingBox {
                    left: rect.left() as f32,
                    top: rect.top() as f32,
                    right: rect.right() as f32,
                    bottom: rect.bottom() as f32,
                },
                // ocrs does not expose per-line confidence; the pipeline treats
                // every block as confident and flags review through other means.
                confidence: 1.0,
                kind: BlockKind::Text,
            });
        }
        Ok(blocks)
    }
}

/// Build the compiled backend, returning a clear error when it is not enabled.
pub fn build(request: &InitRequest) -> Result<Box<dyn OcrEngine>, PerceptionError> {
    #[cfg(not(feature = "ocrs-backend"))]
    {
        let _ = request;
        return Err(PerceptionError::BackendUnavailable);
    }
    #[cfg(feature = "ocrs-backend")]
    {
        let detection_model = request
            .detection_model
            .as_deref()
            .map(|path| {
                rten::Model::load_file(path)
                    .map_err(|error| PerceptionError::Ocr(format!("detection model: {error}")))
            })
            .transpose()?;
        let recognition_model = rten::Model::load_file(&request.recognition_model)
            .map_err(|error| PerceptionError::Ocr(format!("recognition model: {error}")))?;
        let mut params = ocrs::OcrEngineParams {
            detection_model,
            recognition_model: Some(recognition_model),
            ..Default::default()
        };
        if let Some(alphabet) = &request.alphabet {
            params.alphabet = Some(alphabet.clone());
        }
        let inner = ocrs::OcrEngine::new(params)
            .map_err(|error| PerceptionError::Ocr(error.to_string()))?;
        Ok(Box::new(OcrsEngine { inner }))
    }
}
