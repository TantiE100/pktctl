mod live;
#[cfg(test)]
pub(crate) mod scripted;

use std::future::Future;

use ptmp::{Call, Value};

pub use live::LivePacketTracer;

pub trait PacketTracer: Send + Sync + 'static {
    fn call(&self, call: Call) -> impl Future<Output = Result<Value, PtError>> + Send;

    fn version(&self) -> impl Future<Output = Result<String, PtError>> + Send;
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PtError {
    #[error("Packet Tracer is not reachable ({0}); open Packet Tracer and retry")]
    Unreachable(String),
    #[error("{0}; see docs/features/exapp-registration.md")]
    NotRegistered(String),
    #[error("Packet Tracer rejected the request: {0}")]
    Rejected(String),
    #[error("Packet Tracer sent an unexpected reply: {0}")]
    UnexpectedReply(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Transport(String),
}

impl From<ptmp::Error> for PtError {
    fn from(error: ptmp::Error) -> Self {
        match error {
            ptmp::Error::Connect { .. } | ptmp::Error::Closed => {
                Self::Unreachable(error.to_string())
            }
            ptmp::Error::AuthRejected { .. } => Self::NotRegistered(error.to_string()),
            ptmp::Error::Remote { class, message } => Self::Rejected(format!("{class}: {message}")),
            other => Self::Transport(other.to_string()),
        }
    }
}

pub(crate) fn expect_text(value: &Value, what: &str) -> Result<String, PtError> {
    match value.as_str() {
        Some(text) => Ok(text.to_owned()),
        None => Err(PtError::UnexpectedReply(format!(
            "{what} should be text, got {value:?}"
        ))),
    }
}

pub(crate) fn expect_integer(value: &Value, what: &str) -> Result<i64, PtError> {
    value.as_i64().ok_or_else(|| {
        PtError::UnexpectedReply(format!("{what} should be a number, got {value:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_protocol_errors_to_actionable_categories() {
        let remote = ptmp::Error::Remote {
            class: "Device".into(),
            message: "IPC Cache entry: ".into(),
        };
        assert!(matches!(PtError::from(remote), PtError::Rejected(_)));
        assert!(matches!(
            PtError::from(ptmp::Error::Closed),
            PtError::Unreachable(_)
        ));
        let rejected = ptmp::Error::AuthRejected {
            app_id: "app".into(),
        };
        assert!(matches!(PtError::from(rejected), PtError::NotRegistered(_)));
    }

    #[test]
    fn value_helpers_explain_mismatches() {
        assert_eq!(expect_text(&Value::qstring("R1"), "name").unwrap(), "R1");
        let error = expect_integer(&Value::string("x"), "device count").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("device count should be a number")
        );
    }
}
