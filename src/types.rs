use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PerceptionError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("image decode failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("image dimensions exceed configured limits")]
    ImageTooLarge,
    #[error("photo contains no OCR text")]
    EmptyRecognition,
    #[error("OCR adapter failed: {0}")]
    Ocr(String),
    #[error("OCR engine is not initialized; call init first")]
    EngineNotInstalled,
    #[error("OCR backend is not compiled in; rebuild with --features ocrs-backend")]
    BackendUnavailable,
    #[error("global engine lock is poisoned")]
    PoisonedLock,
    #[error("null pointer passed across the C boundary")]
    NullPointer,
}

impl PerceptionError {
    /// Stable, machine-readable code returned inside the JSON envelope.
    pub fn code(&self) -> &'static str {
        match self {
            PerceptionError::Json(_) => "INVALID_JSON",
            PerceptionError::Image(_) => "IMAGE_DECODE",
            PerceptionError::ImageTooLarge => "IMAGE_TOO_LARGE",
            PerceptionError::EmptyRecognition => "EMPTY_RECOGNITION",
            PerceptionError::Ocr(_) => "OCR_FAILED",
            PerceptionError::EngineNotInstalled => "ENGINE_NOT_INITIALIZED",
            PerceptionError::BackendUnavailable => "BACKEND_UNAVAILABLE",
            PerceptionError::PoisonedLock => "ENGINE_LOCKED",
            PerceptionError::NullPointer => "NULL_INPUT",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerceptionRequest {
    /// Stable app-side photo identifier; the raw photo bytes remain local.
    pub photo_id: String,
    /// Encoded JPEG/PNG/WebP bytes as base64, supplied for the native boundary.
    pub image_base64: String,
    /// Optional expected image rotation in clockwise degrees.
    #[serde(default)]
    pub rotation_degrees: u16,
    /// Language/model hint forwarded to the platform OCR adapter.
    #[serde(default = "default_language")]
    pub language: String,
    /// Optional user-selected region of interest in original-image pixels.
    #[serde(default)]
    pub region: Option<BoundingBox>,
}

fn default_language() -> String { "zh-CN".to_owned() }

/// Initialization request for the OCR backend.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitRequest {
    /// Optional path to the text-detection model. Some backends require it.
    #[serde(default)]
    pub detection_model: Option<String>,
    /// Path to the text-recognition model. Required by the `ocrs-backend`.
    pub recognition_model: String,
    /// Backend language hint (not used by the default ocrs English models).
    #[serde(default = "default_language")]
    pub language: String,
    /// Optional recognition alphabet; needed for custom (e.g. CJK) models.
    #[serde(default)]
    pub alphabet: Option<String>,
}

/// User correction applied to one evidence block.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRequest {
    /// Original block as returned inside `PerceptionResult.blocks`.
    pub evidence: EvidenceBlock,
    /// Replacement text, or omitted to keep the recognized text.
    #[serde(default)]
    pub corrected_text: Option<String>,
    /// When true, clears `needsReview` after the user checked the evidence.
    #[serde(default)]
    pub review_cleared: bool,
}

/// Corrected block returned by `m1_correct_json`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectedEvidence {
    pub evidence_id: String,
    pub text: String,
    pub kind: BlockKind,
    pub confidence: f32,
    pub original_bounds: BoundingBox,
    pub reading_order: usize,
    pub needs_review: bool,
    pub user_corrected: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl BoundingBox {
    pub fn valid_for(self, width: u32, height: u32) -> bool {
        self.left.is_finite() && self.top.is_finite()
            && self.right.is_finite() && self.bottom.is_finite()
            && self.left >= 0.0 && self.top >= 0.0
            && self.right <= width as f32 && self.bottom <= height as f32
            && self.left < self.right && self.top < self.bottom
    }

    pub fn center_y(self) -> f32 { (self.top + self.bottom) / 2.0 }
    pub fn center_x(self) -> f32 { (self.left + self.right) / 2.0 }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrBlock {
    pub text: String,
    pub bounds: BoundingBox,
    pub confidence: f32,
    #[serde(default)]
    pub kind: BlockKind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    #[default]
    Text,
    Formula,
    DiagramLabel,
    Handwriting,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBlock {
    /// Stable within a result and deterministic for identical ordered input.
    pub evidence_id: String,
    pub text: String,
    pub kind: BlockKind,
    pub confidence: f32,
    /// Bounds are mapped back to the original, unrotated photograph.
    pub original_bounds: BoundingBox,
    pub reading_order: usize,
    pub needs_review: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerceptionResult {
    pub schema_version: u16,
    pub photo_id: String,
    pub original_width: u32,
    pub original_height: u32,
    pub reading_order: Vec<String>,
    pub blocks: Vec<EvidenceBlock>,
    pub warnings: Vec<String>,
}

/// OCR is a platform adapter: the ocrs backend or a host test double.
pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &image::RgbaImage, language: &str) -> Result<Vec<OcrBlock>, String>;
}
