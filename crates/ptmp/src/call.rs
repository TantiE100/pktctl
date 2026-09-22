use std::fmt;

use crate::{
    error::ProtocolError,
    fields::Fields,
    frame::FrameBuilder,
    value::{TypeCode, Value},
};

const END_OF_ARGUMENTS: &str = "0";

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub method: String,
    pub args: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    steps: Vec<Step>,
}

impl Call {
    pub fn root(accessor: impl Into<String>) -> Self {
        Self::root_with(accessor, [])
    }

    pub fn root_with(accessor: impl Into<String>, args: impl IntoIterator<Item = Value>) -> Self {
        Self {
            steps: vec![Step {
                method: accessor.into(),
                args: args.into_iter().collect(),
            }],
        }
    }

    #[must_use]
    pub fn method(
        mut self,
        name: impl Into<String>,
        args: impl IntoIterator<Item = Value>,
    ) -> Self {
        self.steps.push(Step {
            method: name.into(),
            args: args.into_iter().collect(),
        });
        self
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(crate) fn encode(&self, out: &mut FrameBuilder) -> Result<(), ProtocolError> {
        for step in &self.steps {
            out.text(&step.method);
            for arg in &step.args {
                arg.encode_argument(out)?;
            }
            out.text(END_OF_ARGUMENTS);
        }
        Ok(())
    }

    pub(crate) fn decode(fields: &mut Fields<'_>) -> Result<Self, ProtocolError> {
        let mut steps = Vec::new();
        while !fields.is_empty() {
            let method = fields.next_owned("method")?;
            let mut args = Vec::new();
            loop {
                let code = TypeCode::parse(fields.next("argument type")?)?;
                if code == TypeCode::Void {
                    break;
                }
                args.push(Value::decode_payload(code, fields)?);
            }
            steps.push(Step { method, args });
        }
        if steps.is_empty() {
            return Err(ProtocolError::MissingField("method"));
        }
        Ok(Self { steps })
    }
}

impl fmt::Display for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            write!(f, "{}(", step.method)?;
            for (position, arg) in step.args.iter().enumerate() {
                if position > 0 {
                    f.write_str(", ")?;
                }
                match arg.as_str() {
                    Some(text) => write!(f, "{text:?}")?,
                    None => write!(f, "{arg:?}")?,
                }
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_call() -> Call {
        Call::root("network")
            .method("getDevice", [Value::qstring("R1")])
            .method("getPort", [Value::string("GigabitEthernet0/0.10")])
            .method("getIpAddress", [])
    }

    fn encoded(call: &Call) -> Vec<u8> {
        let mut out = FrameBuilder::default();
        call.encode(&mut out).unwrap();
        out.build().unwrap().body().to_vec()
    }

    #[test]
    fn encodes_like_the_official_framework() {
        let expected: crate::frame::Frame = [
            "network",
            "0",
            "getDevice",
            "9",
            "R1",
            "0",
            "getPort",
            "8",
            "GigabitEthernet0/0.10",
            "0",
            "getIpAddress",
            "0",
        ]
        .into_iter()
        .collect();
        assert_eq!(encoded(&captured_call()), expected.body());
    }

    #[test]
    fn decodes_what_it_encodes() {
        let body = encoded(&captured_call());
        let decoded = Call::decode(&mut Fields::new(&body)).unwrap();
        assert_eq!(decoded, captured_call());
    }

    #[test]
    fn renders_a_readable_path() {
        assert_eq!(
            captured_call().to_string(),
            r#"network().getDevice("R1").getPort("GigabitEthernet0/0.10").getIpAddress()"#
        );
    }

    #[test]
    fn rejects_empty_calls() {
        assert!(Call::decode(&mut Fields::new(&[])).is_err());
    }
}
