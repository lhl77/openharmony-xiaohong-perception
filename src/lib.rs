//! Xiaohong's traceable perception core.
//!
//! OCR is supplied through [`OcrEngine`]. On-device builds enable the
//! `ocrs-backend` feature to load text detection/recognition models; hosts can
//! inject any adapter for tests. The C ABI (`m1_*`) returns a JSON envelope so
//! ArkTS can tell business results from transport failures.

mod ocrs_backend;
mod pipeline;
mod types;

pub use pipeline::{PerceptionConfig, PerceptionEngine, reading_order};
pub use types::*;

use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, RwLock};

static ENGINE: RwLock<Option<PerceptionEngine>> = RwLock::new(None);

/// Install a custom OCR adapter. Useful for host tests and platform fallbacks.
pub fn install_ocr_engine(engine: Arc<dyn OcrEngine>) -> Result<(), PerceptionError> {
    let mut slot = ENGINE.write().map_err(|_| PerceptionError::PoisonedLock)?;
    *slot = Some(PerceptionEngine::new(engine, PerceptionConfig::default()));
    Ok(())
}

/// Build an engine from an [`InitRequest`] JSON string using the compiled backend.
pub fn init_engine(request_json: &str) -> Result<(), PerceptionError> {
    let request: InitRequest = serde_json::from_str(request_json)?;
    let backend = crate::ocrs_backend::build(&request)?;
    install_ocr_engine(Arc::from(backend))
}

/// Process one photo request JSON and return the structured `PerceptionResult`.
pub fn process_photo(request_json: &str) -> Result<PerceptionResult, PerceptionError> {
    let request: PerceptionRequest = serde_json::from_str(request_json)?;
    let slot = ENGINE.read().map_err(|_| PerceptionError::PoisonedLock)?;
    let engine = slot.as_ref().ok_or(PerceptionError::EngineNotInstalled)?;
    engine.process(request)
}

/// Apply a user correction to one evidence block.
pub fn correct_evidence(request_json: &str) -> Result<CorrectedEvidence, PerceptionError> {
    let request: CorrectionRequest = serde_json::from_str(request_json)?;
    let text = request
        .corrected_text
        .as_deref()
        .unwrap_or(&request.evidence.text)
        .to_owned();
    Ok(CorrectedEvidence {
        evidence_id: request.evidence.evidence_id,
        text,
        kind: request.evidence.kind,
        confidence: request.evidence.confidence,
        original_bounds: request.evidence.original_bounds,
        reading_order: request.evidence.reading_order,
        needs_review: !request.review_cleared && request.evidence.needs_review,
        user_corrected: request.review_cleared || request.corrected_text.is_some(),
    })
}

fn envelope<T: serde::Serialize>(result: Result<T, PerceptionError>) -> String {
    match result {
        Ok(data) => serde_json::json!({"ok": true, "data": data}).to_string(),
        Err(error) => serde_json::json!({
            "ok": false,
            "error": {"code": error.code(), "message": error.to_string()}
        })
        .to_string(),
    }
}

/// # Safety
/// `pointer` must be null or a valid NUL-terminated UTF-8 string for this call.
unsafe fn read_input(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let input = unsafe { CStr::from_ptr(pointer) };
    input.to_str().ok().map(str::to_owned)
}

fn into_c(output: String) -> *mut c_char {
    CString::new(output)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Initialize the OCR backend. Returns an envelope on the C boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn m1_init_json(input: *const c_char) -> *mut c_char {
    let output = match unsafe { read_input(input) } {
        None => envelope(Err::<(), _>(PerceptionError::NullPointer)),
        Some(request) => envelope(init_engine(&request)),
    };
    into_c(output)
}

/// Process one photo. Returns an envelope on the C boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn m1_process_json(input: *const c_char) -> *mut c_char {
    let output = match unsafe { read_input(input) } {
        None => envelope(Err::<PerceptionResult, _>(PerceptionError::NullPointer)),
        Some(request) => envelope(process_photo(&request)),
    };
    into_c(output)
}

/// Apply a user correction to one evidence block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn m1_correct_json(input: *const c_char) -> *mut c_char {
    let output = match unsafe { read_input(input) } {
        None => envelope(Err::<CorrectedEvidence, _>(PerceptionError::NullPointer)),
        Some(request) => envelope(correct_evidence(&request)),
    };
    into_c(output)
}

/// Release a string returned by the `m1_*` functions.
///
/// # Safety
/// `value` must be null or a pointer returned by one of the `m1_*` functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn m1_free_string(value: *mut c_char) {
    if !value.is_null() {
        drop(unsafe { CString::from_raw(value) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn call_init(input: *const c_char) -> String {
        let output = unsafe { m1_init_json(input) };
        assert!(!output.is_null(), "envelope must not be null");
        let value = unsafe { CStr::from_ptr(output).to_str().unwrap().to_owned() };
        unsafe { m1_free_string(output) };
        value
    }

    #[test]
    fn null_input_returns_null_input_envelope() {
        let value = unsafe { call_init(std::ptr::null()) };
        assert!(value.contains("\"ok\":false"));
        assert!(value.contains("\"code\":\"NULL_INPUT\""));
    }

    #[test]
    fn invalid_json_reports_invalid_json_code() {
        let input = CString::new("not json").unwrap();
        let value = unsafe { call_init(input.as_ptr()) };
        assert!(value.contains("\"code\":\"INVALID_JSON\""));
    }

    #[cfg(not(feature = "ocrs-backend"))]
    #[test]
    fn init_without_backend_reports_backend_unavailable() {
        let input = CString::new(r#"{"recognitionModel":"models/rec.onnx"}"#).unwrap();
        let value = unsafe { call_init(input.as_ptr()) };
        assert!(value.contains("\"ok\":false"));
        assert!(value.contains("\"code\":\"BACKEND_UNAVAILABLE\""));
    }
}
