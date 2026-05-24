//! Apple Vision OCR — literal text extraction from image bytes.
//!
//! Used by the compose sheet to seed alt-text from screenshots, memes,
//! and any image with visible text. Complements the LLM scene
//! description (alt_text::SmooLlmAltText): the LLM tells the reader
//! *what the image shows*, OCR transcribes *what it says*.
//!
//! Both run in parallel; the compose sheet merges them via
//! [`crate::alt_text::merge_descriptions`].
//!
//! Backend availability:
//! - **macOS** → Apple Vision (`VNRecognizeTextRequest`) — native,
//!   fast (50-300ms on Apple Silicon), free, no network.
//! - **Other platforms** → no-op. Future: optional Tesseract fallback.

use anyhow::Result;

/// Run OCR on the given image bytes (any format Vision can decode —
/// JPEG, PNG, HEIC, TIFF). Returns the recognized text lines in
/// reading order, top to bottom.
pub fn extract_text(image_bytes: &[u8]) -> Result<Vec<String>> {
    backend::extract_text(image_bytes)
}

/// Convenience: run OCR and return a single joined string suitable
/// for an alt-text seed, or `None` if the result was too short to be
/// meaningful (avoids alt-fills like "x" or "Hi").
pub fn extract_text_joined(image_bytes: &[u8]) -> Option<String> {
    let lines = extract_text(image_bytes).ok().unwrap_or_default();
    let joined = lines.join(" ").trim().to_string();
    // Anything under 4 characters is almost certainly noise — a single
    // letter on a sign, a misread punctuation mark. Skip it.
    if joined.chars().count() < 4 {
        None
    } else {
        Some(joined)
    }
}

#[cfg(target_os = "macos")]
mod backend {
    use anyhow::Result;
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    pub fn extract_text(image_bytes: &[u8]) -> Result<Vec<String>> {
        // SAFETY: we only call documented Vision/Foundation APIs on
        // value types we just constructed. NSData is initialized from
        // a freshly-copied byte slice (NSData::with_bytes copies).
        unsafe {
            let request: Retained<VNRecognizeTextRequest> = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);

            let data = NSData::with_bytes(image_bytes);
            let options: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
                NSDictionary::new();
            let handler = VNImageRequestHandler::initWithData_options(
                VNImageRequestHandler::alloc(),
                &data,
                &options,
            );

            let requests: Retained<NSArray<VNRequest>> =
                NSArray::from_retained_slice(&[Retained::cast_unchecked::<VNRequest>(request.clone())]);
            handler
                .performRequests_error(&requests)
                .map_err(|e| anyhow::anyhow!("Vision performRequests failed: {e}"))?;

            let mut out = Vec::new();
            if let Some(results) = request.results() {
                // Each result is a VNRecognizedTextObservation. topCandidates(1)
                // gives the highest-confidence transcription.
                for obs in results.iter() {
                    let candidates = obs.topCandidates(1);
                    if let Some(cand) = candidates.iter().next() {
                        let s = cand.string().to_string();
                        if !s.trim().is_empty() {
                            out.push(s);
                        }
                    }
                }
            }
            Ok(out)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use anyhow::Result;
    pub fn extract_text(_image_bytes: &[u8]) -> Result<Vec<String>> {
        // No-op fallback. A future Tesseract integration would slot in
        // here without touching callers.
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_joined_filters_noise() {
        // 'A' alone is 1 char — below the 4-char floor.
        // We can't easily construct test JPEG bytes that Vision will
        // recognize without an actual image file. So just exercise the
        // joined helper's filtering logic via direct call.
        // (The real `extract_text` is platform-gated; on Linux CI this
        // returns Ok(vec![]) so joined is None — matching expectation.)
        let empty = extract_text_joined(&[]);
        assert!(empty.is_none(), "empty input should yield None");
    }
}
