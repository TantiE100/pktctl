#![doc = include_str!("../README.md")]

use std::io::{Read, Write};

mod eax;
mod physical;

pub use physical::{PhysicalNode, add_building, physical_nodes, rename_node};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

const KEY: [u8; 16] = [0x89; 16];
const NONCE: [u8; 16] = [0x10; 16];
const TAG_LEN: usize = 16;
const SIZE_PREFIX: usize = 4;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PktError {
    #[error("the file is too short to be a Packet Tracer file")]
    TooShort,
    #[error("the file failed its integrity check; it is not a .pkt file or it is damaged")]
    Integrity,
    #[error("the file content is not valid compressed data: {0}")]
    Compression(String),
    #[error("the file content is not UTF-8 XML")]
    NotUtf8,
    #[error("the file is too large to write ({0} bytes)")]
    TooLarge(usize),
    #[error("the file's XML could not be read: {0}")]
    Xml(String),
    #[error("physical location {0} is not in the file")]
    NodeNotFound(String),
}

pub fn decode(file: &[u8]) -> Result<String, PktError> {
    if file.len() < TAG_LEN + SIZE_PREFIX {
        return Err(PktError::TooShort);
    }
    let mut sealed = unscramble(file);
    let tag_start = sealed.len() - TAG_LEN;
    let tag: [u8; TAG_LEN] = sealed[tag_start..]
        .try_into()
        .map_err(|_| PktError::TooShort)?;
    sealed.truncate(tag_start);
    if !eax::open(&KEY, &NONCE, &mut sealed, &tag) {
        return Err(PktError::Integrity);
    }
    let compressed = mask(sealed);
    let xml = inflate(&compressed)?;
    String::from_utf8(xml).map_err(|_| PktError::NotUtf8)
}

pub fn encode(xml: &str) -> Result<Vec<u8>, PktError> {
    let mut sealed = mask(deflate(xml.as_bytes())?);
    let tag = eax::seal(&KEY, &NONCE, &mut sealed);
    sealed.extend_from_slice(&tag);
    Ok(scramble(&sealed))
}

#[allow(clippy::cast_possible_truncation)]
fn low_byte(value: usize) -> u8 {
    (value & 0xFF) as u8
}

fn unscramble(file: &[u8]) -> Vec<u8> {
    let len = file.len();
    (0..len)
        .map(|index| file[len - 1 - index] ^ low_byte(len.wrapping_sub(index.wrapping_mul(len))))
        .collect()
}

fn scramble(sealed: &[u8]) -> Vec<u8> {
    let len = sealed.len();
    let mut file = vec![0; len];
    for (index, byte) in sealed.iter().enumerate() {
        file[len - 1 - index] = byte ^ low_byte(len.wrapping_sub(index.wrapping_mul(len)));
    }
    file
}

fn mask(mut data: Vec<u8>) -> Vec<u8> {
    let len = data.len();
    for (index, byte) in data.iter_mut().enumerate() {
        *byte ^= low_byte(len.wrapping_sub(index));
    }
    data
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, PktError> {
    let (size, body) = data.split_at(SIZE_PREFIX);
    let size = u32::from_be_bytes(size.try_into().map_err(|_| PktError::TooShort)?);
    let mut xml = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
    ZlibDecoder::new(body)
        .read_to_end(&mut xml)
        .map_err(|error| PktError::Compression(error.to_string()))?;
    xml.truncate(usize::try_from(size).unwrap_or(usize::MAX));
    Ok(xml)
}

fn deflate(xml: &[u8]) -> Result<Vec<u8>, PktError> {
    let size = u32::try_from(xml.len()).map_err(|_| PktError::TooLarge(xml.len()))?;
    let mut out = size.to_be_bytes().to_vec();
    let mut encoder = ZlibEncoder::new(&mut out, Compression::default());
    encoder
        .write_all(xml)
        .and_then(|()| encoder.finish().map(drop))
        .map_err(|error| PktError::Compression(error.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrambling_is_reversible() {
        let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        assert_eq!(unscramble(&scramble(&data)), data);
        assert_eq!(mask(mask(data.clone())), data);
    }

    #[test]
    fn round_trips_xml() {
        let xml = "<PACKETTRACER5><VERSION>9.0.1.0858</VERSION><NAME>Ñandú</NAME></PACKETTRACER5>";
        assert_eq!(decode(&encode(xml).unwrap()).unwrap(), xml);
    }

    #[test]
    fn rejects_damaged_files() {
        let mut file = encode("<PACKETTRACER5/>").unwrap();
        file[10] ^= 0xFF;
        assert_eq!(decode(&file), Err(PktError::Integrity));
        assert_eq!(decode(&[1, 2, 3]), Err(PktError::TooShort));
    }
}
