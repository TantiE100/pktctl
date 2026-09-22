use crate::{error::ProtocolError, value::Fields};

const SIGNATURE: &str = "PTMP";
const PROTOCOL_VERSION: i32 = 1;
const TEXT_ENCODING: i32 = 1;
const NO_ENCRYPTION: i32 = 1;
const NO_COMPRESSION: i32 = 1;
const MD5_AUTHENTICATION: i32 = 4;
const VERSION_PREFIX: &str = ":PTVER";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiation {
    pub app_uuid: String,
    pub encoding: i32,
    pub encryption: i32,
    pub compression: i32,
    pub authentication: i32,
    pub timestamp: String,
    pub keepalive_secs: i32,
    pub reserved: String,
}

impl Negotiation {
    pub fn client_request(app_uuid: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            app_uuid: app_uuid.into(),
            encoding: TEXT_ENCODING,
            encryption: NO_ENCRYPTION,
            compression: NO_COMPRESSION,
            authentication: MD5_AUTHENTICATION,
            timestamp: timestamp.into(),
            keepalive_secs: 0,
            reserved: String::new(),
        }
    }

    pub fn pt_version(&self) -> Option<&str> {
        self.reserved.strip_prefix(VERSION_PREFIX)
    }

    #[must_use]
    pub fn with_pt_version(mut self, version: &str) -> Self {
        self.reserved = format!("{VERSION_PREFIX}{version}");
        self
    }

    pub fn unsupported_setting(&self) -> Option<String> {
        [
            ("encoding", self.encoding, TEXT_ENCODING),
            ("encryption", self.encryption, NO_ENCRYPTION),
            ("compression", self.compression, NO_COMPRESSION),
            ("authentication", self.authentication, MD5_AUTHENTICATION),
        ]
        .into_iter()
        .find(|(_, actual, expected)| actual != expected)
        .map(|(name, actual, expected)| format!("{name} is {actual}, pktctl requires {expected}"))
    }

    pub(crate) fn encode(&self, out: &mut Vec<String>) {
        out.extend([
            SIGNATURE.to_owned(),
            PROTOCOL_VERSION.to_string(),
            self.app_uuid.clone(),
            self.encoding.to_string(),
            self.encryption.to_string(),
            self.compression.to_string(),
            self.authentication.to_string(),
            self.timestamp.clone(),
            self.keepalive_secs.to_string(),
            self.reserved.clone(),
        ]);
    }

    pub(crate) fn decode(fields: &mut Fields<'_>) -> Result<Self, ProtocolError> {
        let signature = fields.next("signature")?;
        if signature != SIGNATURE {
            return Err(ProtocolError::InvalidField {
                field: "signature",
                value: signature.to_owned(),
            });
        }
        let version: i32 = fields.parse("protocol version")?;
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidField {
                field: "protocol version",
                value: version.to_string(),
            });
        }
        Ok(Self {
            app_uuid: fields.next_owned("application uuid")?,
            encoding: fields.parse("encoding")?,
            encryption: fields.parse("encryption")?,
            compression: fields.parse("compression")?,
            authentication: fields.parse("authentication")?,
            timestamp: fields.next_owned("timestamp")?,
            keepalive_secs: fields.parse("keepalive")?,
            reserved: fields.next_owned("reserved").unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_response() -> Vec<String> {
        [
            "PTMP",
            "1",
            "{9d706eaf-60d5-4a2b-80fc-0985c27d7994}",
            "1",
            "1",
            "1",
            "4",
            "20260922003430",
            "0",
            ":PTVER9.0.1.0858",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn reads_packet_tracer_version() {
        let negotiation = Negotiation::decode(&mut Fields::new(&captured_response())).unwrap();
        assert_eq!(negotiation.pt_version(), Some("9.0.1.0858"));
        assert_eq!(negotiation.unsupported_setting(), None);
    }

    #[test]
    fn flags_settings_pktctl_cannot_speak() {
        let mut negotiation = Negotiation::client_request("{a}", "20260101000000");
        negotiation.compression = 2;
        assert_eq!(
            negotiation.unsupported_setting().as_deref(),
            Some("compression is 2, pktctl requires 1")
        );
    }

    #[test]
    fn rejects_foreign_signature() {
        let mut fields = captured_response();
        fields[0] = "HTTP".into();
        assert!(Negotiation::decode(&mut Fields::new(&fields)).is_err());
    }

    #[test]
    fn round_trips() {
        let request =
            Negotiation::client_request("{a}", "20260101000000").with_pt_version("9.0.1.0858");
        let mut out = Vec::new();
        request.encode(&mut out);
        assert_eq!(
            Negotiation::decode(&mut Fields::new(&out)).unwrap(),
            request
        );
    }
}
