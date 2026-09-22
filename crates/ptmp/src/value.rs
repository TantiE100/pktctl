use std::net::{Ipv4Addr, Ipv6Addr};

use crate::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeCode {
    Void = 0,
    Byte = 1,
    Bool = 2,
    Short = 3,
    Int = 4,
    Long = 5,
    Float = 6,
    Double = 7,
    String = 8,
    QString = 9,
    Ip = 10,
    Ipv6 = 11,
    Mac = 12,
    Uuid = 13,
    Pair = 14,
    Vector = 15,
}

impl TypeCode {
    const ALL: [Self; 16] = [
        Self::Void,
        Self::Byte,
        Self::Bool,
        Self::Short,
        Self::Int,
        Self::Long,
        Self::Float,
        Self::Double,
        Self::String,
        Self::QString,
        Self::Ip,
        Self::Ipv6,
        Self::Mac,
        Self::Uuid,
        Self::Pair,
        Self::Vector,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub(crate) fn parse(text: &str) -> Result<Self, ProtocolError> {
        text.parse::<u8>()
            .ok()
            .and_then(|code| Self::ALL.get(usize::from(code)).copied())
            .ok_or_else(|| ProtocolError::UnknownTypeCode(text.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Void,
    Byte(i8),
    Bool(bool),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    QString(String),
    Ip(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Mac(String),
    Uuid(String),
    Pair(Box<Value>, Box<Value>),
    Vector {
        element: TypeCode,
        items: Vec<Value>,
    },
}

impl Value {
    pub fn string(text: impl Into<String>) -> Self {
        Self::String(text.into())
    }

    pub fn qstring(text: impl Into<String>) -> Self {
        Self::QString(text.into())
    }

    pub fn type_code(&self) -> TypeCode {
        match self {
            Self::Void => TypeCode::Void,
            Self::Byte(_) => TypeCode::Byte,
            Self::Bool(_) => TypeCode::Bool,
            Self::Short(_) => TypeCode::Short,
            Self::Int(_) => TypeCode::Int,
            Self::Long(_) => TypeCode::Long,
            Self::Float(_) => TypeCode::Float,
            Self::Double(_) => TypeCode::Double,
            Self::String(_) => TypeCode::String,
            Self::QString(_) => TypeCode::QString,
            Self::Ip(_) => TypeCode::Ip,
            Self::Ipv6(_) => TypeCode::Ipv6,
            Self::Mac(_) => TypeCode::Mac,
            Self::Uuid(_) => TypeCode::Uuid,
            Self::Pair(..) => TypeCode::Pair,
            Self::Vector { .. } => TypeCode::Vector,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) | Self::QString(text) | Self::Mac(text) | Self::Uuid(text) => {
                Some(text)
            }
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Byte(number) => Some(number.into()),
            Self::Short(number) => Some(number.into()),
            Self::Int(number) => Some(number.into()),
            Self::Long(number) => Some(number),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(flag) => Some(flag),
            _ => None,
        }
    }

    pub fn into_pair(self) -> Option<(Value, Value)> {
        match self {
            Self::Pair(first, second) => Some((*first, *second)),
            _ => None,
        }
    }

    pub fn into_items(self) -> Option<Vec<Value>> {
        match self {
            Self::Vector { items, .. } => Some(items),
            _ => None,
        }
    }

    pub(crate) fn encode_argument(&self, out: &mut Vec<String>) -> Result<(), ProtocolError> {
        let text = match self {
            Self::Void | Self::Pair(..) | Self::Vector { .. } => {
                return Err(ProtocolError::UnsupportedArgument(self.type_code()));
            }
            Self::Byte(number) => number.to_string(),
            Self::Bool(flag) => flag.to_string(),
            Self::Short(number) => number.to_string(),
            Self::Int(number) => number.to_string(),
            Self::Long(number) => number.to_string(),
            Self::Float(number) => number.to_string(),
            Self::Double(number) => number.to_string(),
            Self::Ip(address) => address.to_string(),
            Self::Ipv6(address) => address.to_string(),
            Self::String(text) | Self::QString(text) | Self::Mac(text) | Self::Uuid(text) => {
                text.clone()
            }
        };
        out.push(self.type_code().code().to_string());
        out.push(text);
        Ok(())
    }

    pub(crate) fn encode_result(&self, out: &mut Vec<String>) {
        match self {
            Self::Void => {}
            Self::Pair(first, second) => {
                out.push(TypeCode::Pair.code().to_string());
                first.encode_result(out);
                second.encode_result(out);
            }
            Self::Vector { element, items } => {
                out.push(TypeCode::Vector.code().to_string());
                out.push(element.code().to_string());
                out.push(items.len().to_string());
                for item in items {
                    let mut encoded = Vec::new();
                    item.encode_result(&mut encoded);
                    out.extend(encoded.into_iter().skip(1));
                }
            }
            scalar => {
                scalar
                    .encode_argument(out)
                    .expect("scalar values always encode");
            }
        }
    }

    pub(crate) fn decode(fields: &mut Fields<'_>) -> Result<Self, ProtocolError> {
        let code = TypeCode::parse(fields.next("value type")?)?;
        Self::decode_payload(code, fields)
    }

    pub(crate) fn decode_payload(
        code: TypeCode,
        fields: &mut Fields<'_>,
    ) -> Result<Self, ProtocolError> {
        Ok(match code {
            TypeCode::Void => Self::Void,
            TypeCode::Byte => Self::Byte(fields.parse("byte")?),
            TypeCode::Bool => Self::Bool(fields.parse("bool")?),
            TypeCode::Short => Self::Short(fields.parse("short")?),
            TypeCode::Int => Self::Int(fields.parse("int")?),
            TypeCode::Long => Self::Long(fields.parse("long")?),
            TypeCode::Float => Self::Float(fields.parse("float")?),
            TypeCode::Double => Self::Double(fields.parse("double")?),
            TypeCode::String => Self::String(fields.next_owned("string")?),
            TypeCode::QString => Self::QString(fields.next_owned("qstring")?),
            TypeCode::Ip => Self::Ip(fields.parse("ip")?),
            TypeCode::Ipv6 => Self::Ipv6(fields.parse("ipv6")?),
            TypeCode::Mac => Self::Mac(fields.next_owned("mac")?),
            TypeCode::Uuid => Self::Uuid(fields.next_owned("uuid")?),
            TypeCode::Pair => Self::Pair(
                Box::new(Self::decode(fields)?),
                Box::new(Self::decode(fields)?),
            ),
            TypeCode::Vector => {
                let element = TypeCode::parse(fields.next("vector element type")?)?;
                let count: usize = fields.parse("vector length")?;
                let items = (0..count)
                    .map(|_| Self::decode_payload(element, fields))
                    .collect::<Result<_, _>>()?;
                Self::Vector { element, items }
            }
        })
    }
}

#[derive(Debug)]
pub(crate) struct Fields<'a> {
    remaining: std::slice::Iter<'a, String>,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(fields: &'a [String]) -> Self {
        Self {
            remaining: fields.iter(),
        }
    }

    pub(crate) fn next(&mut self, field: &'static str) -> Result<&'a str, ProtocolError> {
        self.remaining
            .next()
            .map(String::as_str)
            .ok_or(ProtocolError::MissingField(field))
    }

    pub(crate) fn next_owned(&mut self, field: &'static str) -> Result<String, ProtocolError> {
        self.next(field).map(str::to_owned)
    }

    pub(crate) fn parse<T: std::str::FromStr>(
        &mut self,
        field: &'static str,
    ) -> Result<T, ProtocolError> {
        let text = self.next(field)?;
        text.parse().map_err(|_| ProtocolError::InvalidField {
            field,
            value: text.to_owned(),
        })
    }

    pub(crate) fn peek(&self) -> Option<&'a str> {
        self.remaining.as_slice().first().map(String::as_str)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining.as_slice().is_empty()
    }

    pub(crate) fn finish(self) -> Result<(), ProtocolError> {
        match self.remaining.len() {
            0 => Ok(()),
            extra => Err(ProtocolError::TrailingFields(extra)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(fields: &[&str]) -> Result<Value, ProtocolError> {
        let owned: Vec<String> = fields.iter().map(ToString::to_string).collect();
        let mut cursor = Fields::new(&owned);
        let value = Value::decode(&mut cursor)?;
        cursor.finish()?;
        Ok(value)
    }

    fn encode_result(value: &Value) -> Vec<String> {
        let mut out = Vec::new();
        value.encode_result(&mut out);
        out
    }

    #[test]
    fn decodes_captured_int() {
        assert_eq!(decode(&["4", "11"]).unwrap(), Value::Int(11));
    }

    #[test]
    fn decodes_captured_ip() {
        assert_eq!(
            decode(&["10", "192.168.0.3"]).unwrap(),
            Value::Ip(Ipv4Addr::new(192, 168, 0, 3))
        );
    }

    #[test]
    fn decodes_captured_cli_pair() {
        let value = decode(&["14", "4", "0", "8", "Interface  IP-Address"]).unwrap();
        let (status, output) = value.into_pair().unwrap();
        assert_eq!(status.as_i64(), Some(0));
        assert_eq!(output.as_str(), Some("Interface  IP-Address"));
    }

    #[test]
    fn decodes_captured_empty_vector() {
        let value = decode(&["15", "13", "0"]).unwrap();
        assert_eq!(value.into_items(), Some(Vec::new()));
    }

    #[test]
    fn decodes_vector_items_without_per_item_codes() {
        let value = decode(&["15", "8", "2", "Gig0/0", "Gig0/1"]).unwrap();
        let items = value.into_items().unwrap();
        assert_eq!(items[1].as_str(), Some("Gig0/1"));
    }

    #[test]
    fn rejects_unknown_type_code() {
        assert!(matches!(
            decode(&["99", "x"]),
            Err(ProtocolError::UnknownTypeCode(_))
        ));
    }

    #[test]
    fn rejects_malformed_numbers() {
        assert!(matches!(
            decode(&["4", "eleven"]),
            Err(ProtocolError::InvalidField { .. })
        ));
    }

    #[test]
    fn rejects_truncated_payload() {
        assert!(matches!(
            decode(&["14", "4", "0"]),
            Err(ProtocolError::MissingField(_))
        ));
    }

    #[test]
    fn encodes_arguments_with_their_type_code() {
        let mut out = Vec::new();
        Value::qstring("R1").encode_argument(&mut out).unwrap();
        Value::Int(3).encode_argument(&mut out).unwrap();
        assert_eq!(out, ["9", "R1", "4", "3"]);
    }

    #[test]
    fn refuses_composite_arguments() {
        let pair = Value::Pair(Box::new(Value::Int(1)), Box::new(Value::Int(2)));
        assert!(pair.encode_argument(&mut Vec::new()).is_err());
    }

    #[test]
    fn results_round_trip() {
        let values = [
            Value::Bool(true),
            Value::Pair(Box::new(Value::Int(0)), Box::new(Value::string("ok"))),
            Value::Vector {
                element: TypeCode::String,
                items: vec![Value::string("a"), Value::string("b")],
            },
        ];
        for value in values {
            let encoded = encode_result(&value);
            let fields: Vec<&str> = encoded.iter().map(String::as_str).collect();
            assert_eq!(decode(&fields).unwrap(), value);
        }
    }
}
