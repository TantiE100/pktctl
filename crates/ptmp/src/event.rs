use crate::{
    error::ProtocolError,
    fields::Fields,
    frame::FrameBuilder,
    value::{TypeCode, Value},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub token: String,
    pub class: String,
    pub object_uuid: String,
    pub name: String,
    pub args: Vec<Value>,
}

impl Event {
    pub(crate) fn encode(&self, out: &mut FrameBuilder) {
        for field in [&self.token, &self.class, &self.object_uuid, &self.name] {
            out.text(field);
        }
        for arg in &self.args {
            arg.encode_result(out);
        }
        out.text(&TypeCode::Void.code().to_string());
    }

    pub(crate) fn decode(fields: &mut Fields<'_>) -> Result<Self, ProtocolError> {
        let token = fields.next_owned("event token")?;
        let class = fields.next_owned("event class")?;
        let object_uuid = fields.next_owned("event object uuid")?;
        let name = fields.next_owned("event name")?;
        let mut args = Vec::new();
        loop {
            let code = TypeCode::parse(fields.next("event argument type")?)?;
            if code == TypeCode::Void {
                break;
            }
            args.push(Value::decode_payload(code, fields)?);
        }
        Ok(Self {
            token,
            class,
            object_uuid,
            name,
            args,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub class: String,
    pub object_uuid: String,
    pub event: String,
    pub enabled: bool,
}

impl Subscription {
    pub fn to(
        class: impl Into<String>,
        object_uuid: impl Into<String>,
        event: impl Into<String>,
    ) -> Self {
        Self {
            class: class.into(),
            object_uuid: object_uuid.into(),
            event: event.into(),
            enabled: true,
        }
    }

    pub(crate) fn encode(&self, out: &mut FrameBuilder) {
        out.text(&self.class);
        out.text(&self.object_uuid);
        out.text(&self.event);
        out.text(&self.enabled.to_string());
    }

    pub(crate) fn decode(fields: &mut Fields<'_>) -> Result<Self, ProtocolError> {
        Ok(Self {
            class: fields.next_owned("subscription class")?,
            object_uuid: fields.next_owned("subscription object uuid")?,
            event: fields.next_owned("subscription event")?,
            enabled: fields.parse("subscription enabled")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(values: &[&str]) -> Vec<u8> {
        let frame: crate::frame::Frame = values.iter().collect();
        frame.body().to_vec()
    }

    fn encoded(encode: impl FnOnce(&mut FrameBuilder)) -> Vec<u8> {
        let mut out = FrameBuilder::default();
        encode(&mut out);
        out.build().unwrap().body().to_vec()
    }

    #[test]
    fn decodes_captured_name_changed_event() {
        let captured = body(&[
            "1465458924",
            "Device",
            "{5938e156-cea8-2dc4-0f11-397ef726d4ec}",
            "nameChanged",
            "9",
            "R1X",
            "9",
            "R1",
            "0",
        ]);
        let event = Event::decode(&mut Fields::new(&captured)).unwrap();
        assert_eq!(event.name, "nameChanged");
        assert_eq!(event.args, [Value::qstring("R1X"), Value::qstring("R1")]);
    }

    #[test]
    fn event_round_trips() {
        let event = Event {
            token: "7".into(),
            class: "Device".into(),
            object_uuid: "{uuid}".into(),
            name: "powerChanged".into(),
            args: vec![Value::Bool(false)],
        };
        let out = encoded(|out| event.encode(out));
        assert_eq!(Event::decode(&mut Fields::new(&out)).unwrap(), event);
    }

    #[test]
    fn subscription_matches_captured_wire_format() {
        let out = encoded(|out| Subscription::to("Device", "{uuid}", "nameChanged").encode(out));
        assert_eq!(out, body(&["Device", "{uuid}", "nameChanged", "true"]));
    }
}
