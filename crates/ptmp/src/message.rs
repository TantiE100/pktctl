use crate::{
    call::Call,
    error::{EncodeError, ProtocolError},
    event::{Event, Subscription},
    frame::{Frame, FrameBuilder},
    negotiation::Negotiation,
    value::Value,
};

mod kind {
    pub const NEGOTIATION_REQUEST: &str = "0";
    pub const NEGOTIATION_RESPONSE: &str = "1";
    pub const AUTH_REQUEST: &str = "2";
    pub const AUTH_CHALLENGE: &str = "3";
    pub const AUTH_RESPONSE: &str = "4";
    pub const AUTH_STATUS: &str = "5";
    pub const KEEP_ALIVE: &str = "6";
    pub const DISCONNECT: &str = "7";
    pub const IPC_CALL: &str = "100";
    pub const IPC_ERROR: &str = "101";
    pub const IPC_RESPONSE: &str = "102";
    pub const IPC_EVENT: &str = "103";
    pub const IPC_SUBSCRIBE: &str = "104";
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    NegotiationRequest(Negotiation),
    NegotiationResponse(Negotiation),
    AuthRequest {
        app_id: String,
    },
    AuthChallenge {
        challenge: String,
    },
    AuthResponse {
        app_id: String,
        digest: String,
    },
    AuthStatus {
        accepted: bool,
    },
    KeepAlive,
    Disconnect {
        reason: String,
    },
    IpcCall {
        id: u32,
        call: Call,
    },
    IpcError {
        id: u32,
        class: String,
        message: String,
    },
    IpcResponse {
        id: u32,
        value: Value,
    },
    IpcEvent(Event),
    IpcSubscribe(Subscription),
}

impl Message {
    pub fn to_frame(&self) -> Result<Frame, EncodeError> {
        let mut out = FrameBuilder::default();
        out.text(self.kind());
        match self {
            Self::NegotiationRequest(negotiation) | Self::NegotiationResponse(negotiation) => {
                negotiation.encode(&mut out);
            }
            Self::AuthRequest { app_id } => out.text(app_id),
            Self::AuthChallenge { challenge } => out.text(challenge),
            Self::AuthResponse { app_id, digest } => {
                out.text(app_id);
                out.text(digest);
                out.text("");
            }
            Self::AuthStatus { accepted } => out.text(&accepted.to_string()),
            Self::KeepAlive => {}
            Self::Disconnect { reason } => out.text(reason),
            Self::IpcCall { id, call } => {
                out.text(&id.to_string());
                call.encode(&mut out)?;
            }
            Self::IpcError { id, class, message } => {
                out.text(&id.to_string());
                out.text(class);
                out.text(message);
            }
            Self::IpcResponse { id, value } => {
                out.text(&id.to_string());
                value.encode_result(&mut out);
            }
            Self::IpcEvent(event) => event.encode(&mut out),
            Self::IpcSubscribe(subscription) => subscription.encode(&mut out),
        }
        Ok(out.build()?)
    }

    pub fn from_frame(frame: &Frame) -> Result<Self, ProtocolError> {
        let mut fields = frame.fields();
        let kind = fields.next("message type")?;
        let message = match kind {
            kind::NEGOTIATION_REQUEST => {
                Self::NegotiationRequest(Negotiation::decode(&mut fields)?)
            }
            kind::NEGOTIATION_RESPONSE => {
                Self::NegotiationResponse(Negotiation::decode(&mut fields)?)
            }
            kind::AUTH_REQUEST => Self::AuthRequest {
                app_id: fields.next_owned("app id")?,
            },
            kind::AUTH_CHALLENGE => Self::AuthChallenge {
                challenge: fields.next_owned("challenge")?,
            },
            kind::AUTH_RESPONSE => {
                let app_id = fields.next_owned("app id")?;
                let digest = fields.next_owned("digest")?;
                let _custom = fields.next("custom").unwrap_or_default();
                Self::AuthResponse { app_id, digest }
            }
            kind::AUTH_STATUS => Self::AuthStatus {
                accepted: fields.parse("status")?,
            },
            kind::KEEP_ALIVE => Self::KeepAlive,
            kind::DISCONNECT => Self::Disconnect {
                reason: fields.next_owned("reason").unwrap_or_default(),
            },
            kind::IPC_CALL => Self::IpcCall {
                id: fields.parse("call id")?,
                call: Call::decode(&mut fields)?,
            },
            kind::IPC_ERROR => Self::IpcError {
                id: fields.parse("call id")?,
                class: fields.next_owned("error class")?,
                message: fields.next_owned("error message").unwrap_or_default(),
            },
            kind::IPC_RESPONSE => {
                let id = fields.parse("call id")?;
                let value = if fields.is_empty() {
                    Value::Void
                } else {
                    Value::decode(&mut fields)?
                };
                Self::IpcResponse { id, value }
            }
            kind::IPC_EVENT => Self::IpcEvent(Event::decode(&mut fields)?),
            kind::IPC_SUBSCRIBE => Self::IpcSubscribe(Subscription::decode(&mut fields)?),
            other => return Err(ProtocolError::UnknownMessageType(other.to_owned())),
        };
        fields.finish()?;
        Ok(message)
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::NegotiationRequest(_) => kind::NEGOTIATION_REQUEST,
            Self::NegotiationResponse(_) => kind::NEGOTIATION_RESPONSE,
            Self::AuthRequest { .. } => kind::AUTH_REQUEST,
            Self::AuthChallenge { .. } => kind::AUTH_CHALLENGE,
            Self::AuthResponse { .. } => kind::AUTH_RESPONSE,
            Self::AuthStatus { .. } => kind::AUTH_STATUS,
            Self::KeepAlive => kind::KEEP_ALIVE,
            Self::Disconnect { .. } => kind::DISCONNECT,
            Self::IpcCall { .. } => kind::IPC_CALL,
            Self::IpcError { .. } => kind::IPC_ERROR,
            Self::IpcResponse { .. } => kind::IPC_RESPONSE,
            Self::IpcEvent(_) => kind::IPC_EVENT,
            Self::IpcSubscribe(_) => kind::IPC_SUBSCRIBE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(fields: &[&str]) -> Frame {
        fields.iter().copied().collect()
    }

    fn parse(fields: &[&str]) -> Message {
        Message::from_frame(&frame(fields)).unwrap()
    }

    #[test]
    fn parses_captured_void_response() {
        assert_eq!(
            parse(&["102", "9"]),
            Message::IpcResponse {
                id: 9,
                value: Value::Void
            }
        );
    }

    #[test]
    fn parses_captured_remote_error() {
        assert_eq!(
            parse(&[
                "101",
                "3",
                "Network",
                r#"IPC call "noSuchMethod" not found"#
            ]),
            Message::IpcError {
                id: 3,
                class: "Network".into(),
                message: r#"IPC call "noSuchMethod" not found"#.into()
            }
        );
    }

    #[test]
    fn parses_bare_disconnect() {
        assert_eq!(
            parse(&["7"]),
            Message::Disconnect {
                reason: String::new()
            }
        );
    }

    #[test]
    fn encodes_captured_auth_response() {
        let message = Message::AuthResponse {
            app_id: "dev.tanti.ptprobe".into(),
            digest: "3417B8057B803EBC150BC7DABA451340".into(),
        };
        assert_eq!(
            message.to_frame().unwrap(),
            frame(&[
                "4",
                "dev.tanti.ptprobe",
                "3417B8057B803EBC150BC7DABA451340",
                ""
            ])
        );
    }

    #[test]
    fn rejects_unknown_types() {
        assert!(matches!(
            Message::from_frame(&frame(&["205", "x"])),
            Err(ProtocolError::UnknownMessageType(_))
        ));
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(matches!(
            Message::from_frame(&frame(&["5", "true", "extra"])),
            Err(ProtocolError::TrailingBytes(6))
        ));
    }

    #[test]
    fn every_message_round_trips() {
        let messages = [
            Message::NegotiationRequest(Negotiation::client_request("{a}", "20260101000000")),
            Message::AuthRequest {
                app_id: "app".into(),
            },
            Message::AuthChallenge {
                challenge: "c".into(),
            },
            Message::AuthStatus { accepted: false },
            Message::KeepAlive,
            Message::IpcCall {
                id: 4,
                call: Call::root("network").method("getDeviceCount", []),
            },
            Message::IpcResponse {
                id: 4,
                value: Value::Int(11),
            },
            Message::IpcSubscribe(Subscription::to("Device", "{u}", "nameChanged")),
        ];
        for message in messages {
            let frame = message.to_frame().unwrap();
            assert_eq!(Message::from_frame(&frame).unwrap(), message);
        }
    }
}
