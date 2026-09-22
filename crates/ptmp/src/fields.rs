use crate::error::ProtocolError;

#[derive(Debug)]
pub(crate) struct Fields<'a> {
    rest: &'a [u8],
}

impl<'a> Fields<'a> {
    pub(crate) fn new(body: &'a [u8]) -> Self {
        Self { rest: body }
    }

    pub(crate) fn next(&mut self, field: &'static str) -> Result<&'a str, ProtocolError> {
        let end = self
            .rest
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(ProtocolError::MissingField(field))?;
        let (token, rest) = self.rest.split_at(end);
        self.rest = &rest[1..];
        std::str::from_utf8(token).map_err(|_| ProtocolError::InvalidUtf8(field))
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

    pub(crate) fn raw(
        &mut self,
        len: usize,
        field: &'static str,
    ) -> Result<&'a [u8], ProtocolError> {
        if self.rest.len() < len {
            return Err(ProtocolError::MissingField(field));
        }
        let (bytes, rest) = self.rest.split_at(len);
        self.rest = rest;
        Ok(bytes)
    }

    pub(crate) fn peek(&self) -> Option<&'a str> {
        let end = self.rest.iter().position(|&byte| byte == 0)?;
        std::str::from_utf8(&self.rest[..end]).ok()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    pub(crate) fn finish(self) -> Result<(), ProtocolError> {
        match self.rest.len() {
            0 => Ok(()),
            extra => Err(ProtocolError::TrailingBytes(extra)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_tokens_then_raw_bytes() {
        let mut fields = Fields::new(b"15\x001\x003\x00\x00\xff\x89");
        assert_eq!(fields.next("code").unwrap(), "15");
        assert_eq!(fields.parse::<u8>("element").unwrap(), 1);
        let len: usize = fields.parse("length").unwrap();
        assert_eq!(fields.raw(len, "bytes").unwrap(), b"\x00\xff\x89");
        assert!(fields.finish().is_ok());
    }

    #[test]
    fn unterminated_token_is_missing() {
        assert!(matches!(
            Fields::new(b"abc").next("x"),
            Err(ProtocolError::MissingField("x"))
        ));
    }

    #[test]
    fn short_raw_segment_is_missing() {
        assert!(Fields::new(b"ab").raw(3, "bytes").is_err());
    }

    #[test]
    fn invalid_utf8_is_reported_per_field() {
        assert!(matches!(
            Fields::new(b"\xff\x00").next("name"),
            Err(ProtocolError::InvalidUtf8("name"))
        ));
    }

    #[test]
    fn leftovers_fail_finish() {
        let mut fields = Fields::new(b"a\x00b\x00");
        fields.next("a").unwrap();
        assert!(matches!(
            fields.finish(),
            Err(ProtocolError::TrailingBytes(2))
        ));
    }
}
