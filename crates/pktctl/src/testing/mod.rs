//! In-memory stand-in for the parts of Packet Tracer's IPC API that pktctl uses.
//! It answers calls with the same reply types, argument checks and error texts
//! as Packet Tracer 9.0.1, so feature tests exercise real wire semantics.

mod canvas;

pub use canvas::{Canvas, Remote};
