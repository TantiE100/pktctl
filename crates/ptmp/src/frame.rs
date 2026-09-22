use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{error::FrameError, fields::Fields};

pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;
const MAX_LEN_DIGITS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    body: Vec<u8>,
}

impl Frame {
    pub fn from_body(body: Vec<u8>) -> Self {
        Self { body }
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn fields(&self) -> Fields<'_> {
        Fields::new(&self.body)
    }
}

impl<S: AsRef<str>> FromIterator<S> for Frame {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut builder = FrameBuilder::default();
        for field in iter {
            builder.text(field.as_ref());
        }
        builder
            .build()
            .expect("fields collected into a frame must not contain NUL")
    }
}

#[derive(Debug, Default)]
pub(crate) struct FrameBuilder {
    body: Vec<u8>,
    has_nul_in_text: bool,
}

impl FrameBuilder {
    pub(crate) fn text(&mut self, field: &str) {
        self.has_nul_in_text |= field.as_bytes().contains(&0);
        self.body.extend_from_slice(field.as_bytes());
        self.body.push(0);
    }

    pub(crate) fn raw(&mut self, bytes: &[u8]) {
        self.body.extend_from_slice(bytes);
    }

    pub(crate) fn build(self) -> Result<Frame, FrameError> {
        if self.has_nul_in_text {
            return Err(FrameError::NulInField);
        }
        Ok(Frame::from_body(self.body))
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
        Ok(Some(Frame::from_body(src.split_to(body_len).to_vec())))
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = FrameError;

    fn encode(&mut self, frame: Frame, dst: &mut BytesMut) -> Result<(), FrameError> {
        let prefix = frame.body.len().to_string();
        dst.reserve(prefix.len() + 1 + frame.body.len());
        dst.put_slice(prefix.as_bytes());
        dst.put_u8(0);
        dst.put_slice(&frame.body);
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

    fn text_frame(fields: &[&str]) -> Frame {
        fields.iter().collect()
    }

    #[test]
    fn encodes_like_packet_tracer() {
        let frame = text_frame(&["2", "dev.tanti.ptprobe"]);
        assert_eq!(encode(frame), b"20\x002\x00dev.tanti.ptprobe\x00");
    }

    #[test]
    fn decodes_captured_auth_status() {
        let frames = decode_all(b"7\x005\x00true\x00").unwrap();
        assert_eq!(frames, vec![text_frame(&["5", "true"])]);
    }

    #[test]
    fn decodes_back_to_back_frames() {
        let bytes = b"11\x00102\x002\x004\x0011\x006\x00102\x009\x00";
        let frames = decode_all(bytes).unwrap();
        assert_eq!(
            frames,
            vec![
                text_frame(&["102", "2", "4", "11"]),
                text_frame(&["102", "9"])
            ]
        );
    }

    #[test]
    fn waits_for_incomplete_frames() {
        let mut codec = FrameCodec;
        let mut buffer = BytesMut::from(&b"7\x005\x00tr"[..]);
        assert!(codec.decode(&mut buffer).unwrap().is_none());
        buffer.extend_from_slice(b"ue\x00");
        assert_eq!(
            codec.decode(&mut buffer).unwrap(),
            Some(text_frame(&["5", "true"]))
        );
    }

    #[test]
    fn keeps_binary_bodies_intact() {
        let body = b"102\x001\x0015\x001\x004\x00\x89P\x00G";
        let mut wire = format!("{}\x00", body.len()).into_bytes();
        wire.extend_from_slice(body);
        assert_eq!(decode_all(&wire).unwrap()[0].body(), body);
    }

    #[test]
    fn round_trips_utf8_fields() {
        let frame = text_frame(&["100", "Oficiña"]);
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
    fn builder_refuses_nul_inside_text_but_allows_raw_bytes() {
        let mut text = FrameBuilder::default();
        text.text("a\0b");
        assert!(matches!(text.build(), Err(FrameError::NulInField)));

        let mut raw = FrameBuilder::default();
        raw.text("102");
        raw.raw(b"\x00\x01");
        assert_eq!(raw.build().unwrap().body(), b"102\x00\x00\x01");
    }
}
