//! In-memory stand-in for the parts of Packet Tracer's IPC API that pktctl uses.
//! It answers calls with the same reply types, argument checks and error texts
//! as Packet Tracer 9.0.1, so feature tests exercise real wire semantics.

mod canvas;
mod desktop;

pub use canvas::{ActivityFixture, Canvas, HostAddressing, LinkRecord, Remote};
pub use desktop::FakeDesktop;

/// An absolute path for the system the tests run on, since Windows and Unix
/// disagree on what absolute means.
#[must_use]
pub fn absolute(path: &str) -> String {
    let path = path.trim_start_matches('/');
    if cfg!(windows) {
        format!("C:/{path}")
    } else {
        format!("/{path}")
    }
}
