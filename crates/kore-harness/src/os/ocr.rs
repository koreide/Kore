//! Text recognition over screenshots via the macOS Vision framework.
//!
//! OCR locates elements the way a user sees them: the GPUI window does not expose
//! its content through the accessibility tree.

use anyhow::{anyhow, Result};
use objc2::rc::{autoreleasepool, Retained};
use objc2::AllocAnyThread;
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
};
use serde::Serialize;
use std::path::Path;

/// A recognized line of text, in image pixel coordinates (origin top-left).
#[derive(Debug, Clone, Serialize)]
pub struct TextBox {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub confidence: f32,
}

pub fn recognize(path: &Path) -> Result<Vec<TextBox>> {
    // Vision autoreleases the objects that hold the image file open; without a pool they are
    // never drained, so every call leaked a descriptor (inherited by spawned `curl`s, which fail
    // once descriptors pass FD_SETSIZE).
    autoreleasepool(|_| recognize_inner(path))
}

fn recognize_inner(path: &Path) -> Result<Vec<TextBox>> {
    let (img_w, img_h) = image::image_dimensions(path)?;
    let (img_w, img_h) = (img_w as f64, img_h as f64);
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("non-utf8 path {}", path.display()))?;
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
        let handler = VNImageRequestHandler::initWithURL_options(
            VNImageRequestHandler::alloc(),
            &url,
            &NSDictionary::new(),
        );
        let req = VNRecognizeTextRequest::init(VNRecognizeTextRequest::alloc());
        req.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        // Language correction "fixes" identifiers like pod names; keep raw text.
        req.setUsesLanguageCorrection(false);
        // Pin the Latin model: with auto-detection, sparse screens (e.g. the resource
        // graph) are sometimes read as upside-down Cyrillic gibberish.
        req.setRecognitionLanguages(&NSArray::from_retained_slice(&[NSString::from_str("en-US")]));
        req.setRevision(2);
        let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(req.clone()));
        let reqs = NSArray::from_retained_slice(&[as_request]);
        handler
            .performRequests_error(&reqs)
            .map_err(|e| anyhow!("vision OCR failed: {e:?}"))?;

        let mut out = Vec::new();
        if let Some(results) = req.results() {
            for obs in results.iter() {
                let Some(candidate) = obs.topCandidates(1).firstObject() else { continue };
                // Vision boxes are normalized with a bottom-left origin.
                let bb = obs.boundingBox();
                out.push(TextBox {
                    text: candidate.string().to_string(),
                    x: bb.origin.x * img_w,
                    y: (1.0 - bb.origin.y - bb.size.height) * img_h,
                    w: bb.size.width * img_w,
                    h: bb.size.height * img_h,
                    confidence: candidate.confidence(),
                });
            }
        }
        Ok(out)
    }
}
