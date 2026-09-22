use std::net::{Ipv4Addr, Ipv6Addr};

use crate::{error::ProtocolError, fields::Fields, frame::FrameBuilder};

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
    Data = 16,
}

impl TypeCode {
    const ALL: [Self; 17] = [
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
        Self::Data,
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

    fn text(self) -> String {
        self.code().to_string()
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
    Bytes(Vec<u8>),
    /// A value object such as an ACL statement or a flowchart node: its class and its fields.
    Data {
        class: String,
        fields: Vec<Value>,
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
            Self::Vector { .. } | Self::Bytes(_) => TypeCode::Vector,
            Self::Data { .. } => TypeCode::Data,
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

    pub fn as_ip(&self) -> Option<Ipv4Addr> {
        match *self {
            Self::Ip(address) => Some(address),
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

    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub(crate) fn encode_argument(&self, out: &mut FrameBuilder) -> Result<(), ProtocolError> {
        let text = self
            .scalar_text()
            .ok_or(ProtocolError::UnsupportedArgument(self.type_code()))?;
        out.text(&self.type_code().text());
        out.text(&text);
        Ok(())
    }

    pub(crate) fn encode_result(&self, out: &mut FrameBuilder) {
        if matches!(self, Self::Void) {
            return;
        }
        out.text(&self.type_code().text());
        self.encode_payload(out);
    }

    fn encode_payload(&self, out: &mut FrameBuilder) {
        match self {
            Self::Void => {}
            Self::Pair(first, second) => {
                first.encode_result(out);
                second.encode_result(out);
            }
            Self::Vector { element, items } => {
                out.text(&element.text());
                out.text(&items.len().to_string());
                for item in items {
                    item.encode_payload(out);
                }
            }
            Self::Bytes(bytes) => {
                out.text(&TypeCode::Byte.text());
                out.text(&bytes.len().to_string());
                out.raw(bytes);
            }
            Self::Data { class, fields } => {
                out.text(class);
                for field in fields {
                    field.encode_result(out);
                }
            }
            scalar => out.text(&scalar.scalar_text().unwrap_or_default()),
        }
    }

    fn scalar_text(&self) -> Option<String> {
        Some(match self {
            Self::Void
            | Self::Pair(..)
            | Self::Vector { .. }
            | Self::Bytes(_)
            | Self::Data { .. } => return None,
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
        })
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
                if element == TypeCode::Byte {
                    return Ok(Self::Bytes(fields.raw(count, "byte list")?.to_vec()));
                }
                let items = (0..count)
                    .map(|_| Self::decode_payload(element, fields))
                    .collect::<Result<_, _>>()?;
                Self::Vector { element, items }
            }
            TypeCode::Data => {
                let class = fields.next_owned("data class")?;
                if let Some(address) = address_object(&class) {
                    return Ok(address);
                }
                let mut values = Vec::new();
                match fields.layouts().layout(&class) {
                    crate::data::Layout::Fixed(count) => {
                        for _ in 0..count {
                            values.push(Self::decode(fields)?);
                        }
                    }
                    crate::data::Layout::Variable => {
                        while fields
                            .peek()
                            .is_some_and(|token| TypeCode::parse(token).is_ok())
                        {
                            values.push(Self::decode(fields)?);
                        }
                    }
                    crate::data::Layout::NotAClass => return Ok(Self::String(class)),
                }
                Self::Data {
                    class,
                    fields: values,
                }
            }
        })
    }
}

/// Value objects carry their addresses as type 16 with the address as the class token.
fn address_object(token: &str) -> Option<Value> {
    if let Ok(ip) = token.parse::<Ipv4Addr>() {
        return Some(Value::Ip(ip));
    }
    if token.contains(':')
        && let Ok(ip) = token.parse::<Ipv6Addr>()
    {
        return Some(Value::Ipv6(ip));
    }
    let groups: Vec<&str> = token.split('.').collect();
    let mac = groups.len() == 3
        && groups
            .iter()
            .all(|group| group.len() == 4 && group.chars().all(|digit| digit.is_ascii_hexdigit()));
    mac.then(|| Value::Mac(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_body(body: &[u8]) -> Result<Value, ProtocolError> {
        decode_body_with(body, &crate::data::DataLayouts::new())
    }

    fn decode_body_with(
        body: &[u8],
        layouts: &crate::data::DataLayouts,
    ) -> Result<Value, ProtocolError> {
        let mut fields = Fields::with_layouts(body, layouts);
        let value = Value::decode(&mut fields)?;
        fields.finish()?;
        Ok(value)
    }

    fn decode(tokens: &[&str]) -> Result<Value, ProtocolError> {
        let frame: crate::frame::Frame = tokens.iter().collect();
        decode_body(frame.body())
    }

    fn decode_with(
        tokens: &[&str],
        layouts: &[(&str, Option<usize>)],
    ) -> Result<Value, ProtocolError> {
        let frame: crate::frame::Frame = tokens.iter().collect();
        let layouts: crate::data::DataLayouts = layouts.iter().copied().collect();
        decode_body_with(frame.body(), &layouts)
    }

    fn encode_result(value: &Value) -> Vec<u8> {
        let mut out = FrameBuilder::default();
        value.encode_result(&mut out);
        out.build().unwrap().body().to_vec()
    }

    #[test]
    fn addresses_inside_value_objects_decode_as_addresses() {
        let record = decode(&[
            "16",
            "DnsRrATest",
            "8",
            "www.gamc.bo",
            "16",
            "192.168.10.5",
            "16",
            "0001.C734.91D5",
        ])
        .unwrap();
        let Value::Data { fields, .. } = record else {
            panic!("expected data");
        };
        assert_eq!(fields[1], Value::Ip("192.168.10.5".parse().unwrap()));
        assert_eq!(fields[2], Value::Mac("0001.C734.91D5".into()));
    }

    #[test]
    fn decodes_data_objects_by_layout_or_by_type_codes() {
        let node = [
            "16",
            "FlowChartNodeTest",
            "8",
            "CPingProcess_next_ping",
            "9",
            "The Ping process starts.",
            "2",
            "false",
            "4",
            "3",
        ];
        let greedy = decode(&node).unwrap();
        let Value::Data { class, fields } = greedy else {
            panic!("expected data");
        };
        assert_eq!(class, "FlowChartNodeTest");
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3], Value::Int(3));

        let layouts = [("PairedDataTest", Some(1))];
        let paired = decode_with(
            &["14", "16", "PairedDataTest", "8", "x", "4", "7"],
            &layouts,
        )
        .unwrap();
        let (first, second) = paired.into_pair().unwrap();
        assert_eq!(
            first,
            Value::Data {
                class: "PairedDataTest".into(),
                fields: vec![Value::string("x")]
            }
        );
        assert_eq!(second, Value::Int(7));

        let text = decode_with(&["14", "16", "www.gamc.bo", "4", "7"], &layouts).unwrap();
        assert_eq!(text.into_pair().unwrap().0, Value::string("www.gamc.bo"));
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
    fn decodes_captured_png_byte_list_raw() {
        let value = decode_body(b"15\x001\x008\x00\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(value.into_bytes().unwrap(), b"\x89PNG\r\n\x1a\n");
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
        let mut out = FrameBuilder::default();
        Value::qstring("R1").encode_argument(&mut out).unwrap();
        Value::Int(3).encode_argument(&mut out).unwrap();
        assert_eq!(out.build().unwrap().body(), b"9\x00R1\x004\x003\x00");
    }

    #[test]
    fn refuses_composite_arguments() {
        let pair = Value::Pair(Box::new(Value::Int(1)), Box::new(Value::Int(2)));
        assert!(pair.encode_argument(&mut FrameBuilder::default()).is_err());
        assert!(
            Value::Bytes(vec![1])
                .encode_argument(&mut FrameBuilder::default())
                .is_err()
        );
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
            Value::Bytes(vec![0x89, 0, 0xff]),
        ];
        for value in values {
            assert_eq!(decode_body(&encode_result(&value)).unwrap(), value);
        }
    }
}
