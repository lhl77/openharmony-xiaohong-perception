/** Type package for the native libxiaohong_perception.so module. */
export interface InitOptions {
  /** Sandbox-readable absolute path to an ocrs-compatible detection model. */
  detectionModel?: string;
  /** Sandbox-readable absolute path to an ocrs-compatible recognition model. */
  recognitionModel: string;
  /** Defaults to en in Rust. */
  language?: string;
  /** Must match the recognition model's character order. */
  alphabet?: string;
}

export interface M1Error {
  code: string;
  message: string;
}

/** Inspect ok before data/error; business failures still resolve. */
export interface Envelope<T> {
  ok: boolean;
  data?: T;
  error?: M1Error;
}

export interface PerceptionRequest {
  photoId: string;
  /** Existing JPEG/PNG/WebP base64 text. This package does not encode images. */
  imageBase64: string;
  rotationDegrees?: number;
  language?: string;
}

export interface PerceptionNative {
  /** JSON: InitOptions. Only ocrs-compatible models, not arbitrary ONNX. */
  init(requestJson: string): Promise<string>;
  /** JSON: PerceptionRequest. Await successful init first. */
  processPhoto(requestJson: string): Promise<string>;
  /** JSON: correction request defined by the Rust core. */
  correctEvidence(requestJson: string): Promise<string>;
}

/** Exactly one UTF-8 JSON string, at most 32 MiB, without embedded NUL.
 * Type/range errors throw synchronously; native failures reject the Promise.
 * Resolved strings are unchanged Rust envelopes, including business errors.
 */
declare const perception: PerceptionNative;
export default perception;