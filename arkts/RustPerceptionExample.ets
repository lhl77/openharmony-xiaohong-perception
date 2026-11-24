import perception, { InitOptions, PerceptionRequest } from 'libxiaohong_perception.so';

// Caller supplies existing base64 text; no camera, platform OCR or encoding API.
// Return the envelope unchanged so the application can handle business errors.
export async function initializeRustOcr(detectionModel: string,
  recognitionModel: string, alphabet?: string): Promise<string> {
  const options: InitOptions = {
    detectionModel: detectionModel,
    recognitionModel: recognitionModel,
    language: 'en'
  };
  if (alphabet !== undefined) options.alphabet = alphabet;
  return await perception.init(JSON.stringify(options));
}

// Call after awaiting init and inspecting the returned envelope's ok field.
export async function processEncodedPhoto(photoId: string,
  existingImageBase64: string): Promise<string> {
  const request: PerceptionRequest = {
    photoId: photoId,
    imageBase64: existingImageBase64,
    language: 'en'
  };
  return await perception.processPhoto(JSON.stringify(request));
}

// Caller constructs this JSON according to the Rust correction schema.
export async function correctExistingEvidence(correctionJson: string): Promise<string> {
  return await perception.correctEvidence(correctionJson);
}