#![doc = include_str!("../README.md")]

mod auth;
mod call;
mod data;
mod error;
mod event;
mod fields;
mod frame;
mod message;
mod negotiation;
mod session;
mod timestamp;
mod value;

#[cfg(feature = "fake")]
pub mod fake;

pub use auth::md5_digest;
pub use call::{Call, Step};
pub use data::DataLayouts;
pub use error::{EncodeError, Error, FrameError, ProtocolError};
pub use event::{Event, Subscription};
pub use frame::{Frame, FrameCodec, MAX_FRAME_LEN};
pub use message::Message;
pub use negotiation::Negotiation;
pub use session::{Credentials, Session, SessionConfig};
pub use value::{TypeCode, Value};
