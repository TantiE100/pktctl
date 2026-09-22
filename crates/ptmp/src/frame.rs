use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::FrameError;

pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;
const MAX_LEN_DIGITS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    fields: Vec<String>,
}

impl Frame {
    pub fn new(fields: Vec<String>) -> Self {
        Self { fields }
    }

    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    pub fn into_fields(self) -> Vec<String> {
        self.fields
    }
}

impl<S: Into<String>> FromIterator<S> for Frame {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::new(iter.into_iter().map(Into::into).collect())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FrameCodec;

impl Decoder for FrameCodec {
    type Item = Frame;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, FrameError> {
        let head = &src[..src.len().min(MAX_LEN_DIGITS + 1)];
        let separator = match head.iter().position(|&byte| byte == 0) {
            Some(index) => index,
            None if head.len() > MAX_LEN_DIGITS => return Err(FrameError::InvalidLength),
            None => return Ok(None),
        };

        let body_len = parse_length(&src[..separator])?;
        if body_len > MAX_FRAME_LEN {
            return Err(FrameError::TooLarge(body_len));
        }

        let frame_len = separator + 1 + body_len;
        if src.len() < frame_len {
            src.reserve(frame_len - src.len());
            return Ok(None);
        }

        src.advance(separator + 1);
        let body = src.split_to(body_len);
        parse_body(&body).map(Some)
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = FrameError;

    fn encode(&mut self, frame: Frame, dst: &mut BytesMut) -> Result<(), FrameError> {
        let mut body = Vec::new();
        for field in frame.fields() {
            if field.as_bytes().contains(&0) {
                return Err(FrameError::NulInField);
            }
            body.extend_from_slice(field.as_bytes());
            body.push(0);
        }

        let prefix = body.len().to_string();
        dst.reserve(prefix.len() + 1 + body.len());
        dst.put_slice(prefix.as_bytes());
        dst.put_u8(0);
        dst.put_slice(&body);
        Ok(())
    }
}

fn parse_length(digits: &[u8]) -> Result<usize, FrameError> {
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(FrameError::InvalidLength);
    }
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or(FrameError::InvalidLength)
}

fn parse_body(body: &[u8]) -> Result<Frame, FrameError> {
    let Some((&0, fields)) = body.split_last() else {
        return Err(FrameError::MalformedBody);
    };
    fields
        .split(|&byte| byte == 0)
        .map(|field| String::from_utf8(field.to_vec()).map_err(|_| FrameError::InvalidUtf8))
        .collect::<Result<Vec<_>, _>>()
        .map(Frame::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(bytes: &[u8]) -> Result<Vec<Frame>, FrameError> {
        let mut codec = FrameCodec;
        let mut buffer = BytesMut::from(bytes);
        let mut frames = Vec::new();
        while let Some(frame) = codec.decode(&mut buffer)? {
            frames.push(frame);
        }
        Ok(frames)
    }

    fn encode(frame: Frame) -> Vec<u8> {
        let mut buffer = BytesMut::new();
        FrameCodec.encode(frame, &mut buffer).unwrap();
        buffer.to_vec()
    }

    #[test]
    fn encodes_like_packet_tracer() {
        let frame: Frame = ["2", "dev.tanti.ptprobe"].into_iter().collect();
        assert_eq!(encode(frame), b"20\x002\x00dev.tanti.ptprobe\x00");
    }

    #[test]
    fn decodes_captured_auth_status() {
        let frames = decode_all(b"7\x005\x00true\x00").unwrap();
        assert_eq!(frames, vec![["5", "true"].into_iter().collect()]);
    }

    #[test]
    fn decodes_back_to_back_frames() {
        let bytes = b"11\x00102\x002\x004\x0011\x006\x00102\x009\x00";
        let frames = decode_all(bytes).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].fields(), ["102", "9"]);
    }

    #[test]
    fn waits_for_incomplete_frames() {
        let mut codec = FrameCodec;
        let mut buffer = BytesMut::from(&b"7\x005\x00tr"[..]);
        assert!(codec.decode(&mut buffer).unwrap().is_none());
        buffer.extend_from_slice(b"ue\x00");
        assert_eq!(
            codec.decode(&mut buffer).unwrap().unwrap().fields(),
            ["5", "true"]
        );
    }

    #[test]
    fn keeps_empty_fields() {
        let frames = decode_all(b"4\x004\x00\x00\x00").unwrap();
        assert_eq!(frames[0].fields(), ["4", "", ""]);
    }

    #[test]
    fn round_trips_utf8_fields() {
        let frame: Frame = ["100", "Oficiña"].into_iter().collect();
        assert_eq!(decode_all(&encode(frame.clone())).unwrap(), vec![frame]);
    }

    #[test]
    fn rejects_non_numeric_length() {
        assert!(matches!(
            decode_all(b"x1\x00"),
            Err(FrameError::InvalidLength)
        ));
    }

    #[test]
    fn rejects_missing_length_terminator() {
        assert!(matches!(
            decode_all(b"12345678901"),
            Err(FrameError::InvalidLength)
        ));
    }

    #[test]
    fn rejects_oversized_frames() {
        let bytes = format!("{}\x00", MAX_FRAME_LEN + 1);
        assert!(matches!(
            decode_all(bytes.as_bytes()),
            Err(FrameError::TooLarge(_))
        ));
    }

    #[test]
    fn rejects_unterminated_body() {
        assert!(matches!(
            decode_all(b"1\x005"),
            Err(FrameError::MalformedBody)
        ));
    }

    #[test]
    fn refuses_to_encode_nul_inside_field() {
        let frame: Frame = ["100", "a\0b"].into_iter().collect();
        let result = FrameCodec.encode(frame, &mut BytesMut::new());
        assert!(matches!(result, Err(FrameError::NulInField)));
    }
}
