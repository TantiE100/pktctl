use std::time::Duration;

use crate::value::TypeCode;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame length prefix is not a valid decimal number")]
    InvalidLength,
    #[error("frame of {0} bytes exceeds the maximum allowed size")]
    TooLarge(usize),
    #[error("field contains a NUL byte and cannot be encoded")]
    NulInField,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("unknown message type {0}")]
    UnknownMessageType(String),
    #[error("message is missing field `{0}`")]
    MissingField(&'static str),
    #[error("field `{field}` has invalid value {value:?}")]
    InvalidField { field: &'static str, value: String },
    #[error("unknown value type code {0}")]
    UnknownTypeCode(String),
    #[error("{0:?} values cannot be sent as call arguments")]
    UnsupportedArgument(TypeCode),
    #[error("field `{0}` is not valid UTF-8")]
    InvalidUtf8(&'static str),
    #[error("message has {0} unexpected trailing bytes")]
    TrailingBytes(usize),
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Frame(#[from] FrameError),
}

impl From<EncodeError> for Error {
    fn from(error: EncodeError) -> Self {
        match error {
            EncodeError::Protocol(error) => Self::Protocol(error),
            EncodeError::Frame(error) => Self::Frame(error),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach Packet Tracer at {addr}: {source}")]
    Connect {
        addr: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("Packet Tracer did not accept the connection settings: {0}")]
    Negotiation(String),
    #[error(
        "Packet Tracer rejected app id `{app_id}`; register the ExApp and check the shared secret"
    )]
    AuthRejected { app_id: String },
    #[error("expected {expected} but received {received}")]
    Unexpected {
        expected: &'static str,
        received: String,
    },
    #[error("{class}: {message}")]
    Remote { class: String, message: String },
    #[error("no reply from Packet Tracer within {0:?}")]
    Timeout(Duration),
    #[error("connection to Packet Tracer is closed")]
    Closed,
}
