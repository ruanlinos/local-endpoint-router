mod collector;
mod preview;

pub use collector::{TrafficCollector, TrafficEvent, PREVIEW_BYTES};
pub use preview::PreviewBody;

pub fn capture_limit(capture_body: bool, body_limit_bytes: u32) -> usize {
    if capture_body {
        (body_limit_bytes as usize).clamp(PREVIEW_BYTES, 1024 * 1024)
    } else {
        PREVIEW_BYTES
    }
}
