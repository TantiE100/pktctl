use ptmp::{Step, TypeCode, Value};

use crate::packet_tracer::PtError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub class: String,
    pub message: String,
}

impl Remote {
    pub(super) fn new(class: &str, message: impl Into<String>) -> Self {
        Self {
            class: class.to_owned(),
            message: message.into(),
        }
    }

    pub(super) fn missing(class: &str) -> Self {
        Self::new(class, "IPC Cache entry: ")
    }

    pub(super) fn unknown_method(class: &str, method: &str) -> Self {
        Self::new(class, format!(r#"IPC call "{method}" not found"#))
    }

    pub(super) fn invalid_arguments(class: &str, method: &str) -> Self {
        Self::new(
            class,
            format!(r#"Invalid arguments for IPC call "{method}""#),
        )
    }
}

impl From<Remote> for PtError {
    fn from(remote: Remote) -> Self {
        ptmp::Error::Remote {
            class: remote.class,
            message: remote.message,
        }
        .into()
    }
}

pub(super) fn check_args(step: &Step, class: &str, expected: &[TypeCode]) -> Result<(), Remote> {
    let actual: Vec<TypeCode> = step.args.iter().map(Value::type_code).collect();
    if actual == expected {
        Ok(())
    } else {
        Err(Remote::invalid_arguments(class, &step.method))
    }
}

pub(super) fn no_args(step: &Step, class: &str) -> Result<(), Remote> {
    check_args(step, class, &[])
}

pub(super) fn int_arg(step: &Step, class: &str) -> Result<i64, Remote> {
    check_args(step, class, &[TypeCode::Int])?;
    Ok(step.args[0].as_i64().unwrap_or_default())
}

pub(super) fn qstring_arg<'a>(step: &'a Step, class: &str) -> Result<&'a str, Remote> {
    check_args(step, class, &[TypeCode::QString])?;
    Ok(step.args[0].as_str().unwrap_or_default())
}

pub(super) fn string_arg<'a>(step: &'a Step, class: &str) -> Result<&'a str, Remote> {
    check_args(step, class, &[TypeCode::String])?;
    Ok(step.args[0].as_str().unwrap_or_default())
}

pub(super) fn number(value: &Value) -> f64 {
    match *value {
        Value::Double(number) => number,
        Value::Int(number) => f64::from(number),
        _ => 0.0,
    }
}

pub(super) fn count(len: usize) -> Value {
    Value::Int(i32::try_from(len).expect("test canvases stay small"))
}
