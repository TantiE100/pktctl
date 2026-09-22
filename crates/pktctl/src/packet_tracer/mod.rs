mod command;
pub mod kinds;
mod live;
#[cfg(test)]
pub(crate) mod scripted;

use std::future::Future;

use ptmp::{Call, Event, Subscription, Value};
use tokio::sync::broadcast;

pub use command::CommandStatus;
pub use live::LivePacketTracer;

pub type Events = broadcast::Receiver<Event>;

const MISSING_OBJECT: &str = "IPC Cache entry";
const MISSING_PRIVILEGE: &str = "necessary privilege";

pub trait PacketTracer: Send + Sync + 'static {
    fn call(&self, call: Call) -> impl Future<Output = Result<Value, PtError>> + Send;

    fn version(&self) -> impl Future<Output = Result<String, PtError>> + Send;

    fn subscribe(
        &self,
        subscription: Subscription,
    ) -> impl Future<Output = Result<Events, PtError>> + Send;

    fn unsubscribe(
        &self,
        subscription: Subscription,
    ) -> impl Future<Output = Result<(), PtError>> + Send;
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PtError {
    #[error("Packet Tracer is not reachable ({0}); open Packet Tracer and retry")]
    Unreachable(String),
    #[error("{0}; call the setup_exapp tool to create the registration file")]
    NotRegistered(String),
    #[error("Packet Tracer rejected the request: {0}")]
    Rejected(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error(
        "{0}; register the ExApp again with every privilege listed in docs/features/pktctl-exapp.xml"
    )]
    MissingPrivilege(String),
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
            ptmp::Error::Remote { class, message } if message.starts_with(MISSING_OBJECT) => {
                Self::NotFound(class)
            }
            ptmp::Error::Remote { message, .. } if message.contains(MISSING_PRIVILEGE) => {
                Self::MissingPrivilege(message)
            }
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

pub(crate) fn expect_number(value: &Value, what: &str) -> Result<f64, PtError> {
    match *value {
        Value::Double(number) => Ok(number),
        Value::Float(number) => Ok(f64::from(number)),
        Value::Int(number) => Ok(f64::from(number)),
        _ => Err(PtError::UnexpectedReply(format!(
            "{what} should be a number, got {value:?}"
        ))),
    }
}

pub(crate) fn expect_bool(value: &Value, what: &str) -> Result<bool, PtError> {
    value.as_bool().ok_or_else(|| {
        PtError::UnexpectedReply(format!("{what} should be true or false, got {value:?}"))
    })
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
        assert_eq!(PtError::from(remote), PtError::NotFound("Device".into()));
        let refused = ptmp::Error::Remote {
            class: "Network".into(),
            message: r#"IPC call "x" not found"#.into(),
        };
        assert!(matches!(PtError::from(refused), PtError::Rejected(_)));
        assert!(matches!(
            PtError::from(ptmp::Error::Closed),
            PtError::Unreachable(_)
        ));
        let rejected = ptmp::Error::AuthRejected {
            app_id: "app".into(),
        };
        assert!(matches!(PtError::from(rejected), PtError::NotRegistered(_)));
        let denied = ptmp::Error::Remote {
            class: "AppWindow".into(),
            message: r#"ExApp or Script Module does not have the necessary privilege for IPC call "fileOpen""#.into(),
        };
        assert!(
            PtError::from(denied)
                .to_string()
                .contains("pktctl-exapp.xml")
        );
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
