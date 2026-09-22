use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{desktop::Desktop, packet_tracer::PtError};

/// A desktop whose "window" is a fixed PNG, counting captures.
#[derive(Debug, Default)]
pub struct FakeDesktop {
    captures: AtomicUsize,
}

impl FakeDesktop {
    pub const PNG: &'static [u8] = b"\x89PNG\r\n\x1a\nfake-window";

    pub fn captures(&self) -> usize {
        self.captures.load(Ordering::Relaxed)
    }
}

impl Desktop for FakeDesktop {
    fn capture_packet_tracer(&self) -> Result<Vec<u8>, PtError> {
        self.captures.fetch_add(1, Ordering::Relaxed);
        Ok(Self::PNG.to_vec())
    }
}
