//! Independent, bounded compression frames. Decoding provides no authentication.
use crate::{Error, MAX_BLOCK, MAX_ENCODED, MAX_OFFSET, Result};
use flate2::{Compression, Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    None,
    Zlib,
}

// Deliberately no Debug: blocks can contain private command input or output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodedBlock {
    pub offset: u64,
    pub codec: Codec,
    pub raw_len: u32,
    #[serde(with = "crate::wire::hex32")]
    pub sha256: [u8; 32],
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

impl EncodedBlock {
    pub fn end(&self) -> Result<u64> {
        self.offset
            .checked_add(u64::from(self.raw_len))
            .filter(|end| *end <= MAX_OFFSET)
            .ok_or(Error::Offset)
    }

    pub fn validate(&self) -> Result<()> {
        if self.raw_len == 0
            || self.raw_len as usize > MAX_BLOCK
            || self.data.is_empty()
            || self.data.len() > MAX_ENCODED
        {
            return Err(Error::Limit);
        }
        self.end()?;
        if self.codec == Codec::None && self.data.len() != self.raw_len as usize {
            return Err(Error::Codec);
        }
        if self.codec == Codec::Zlib && self.data.len() >= self.raw_len as usize {
            return Err(Error::Codec);
        }
        Ok(())
    }
}

pub fn encode(offset: u64, raw: &[u8], negotiated: Codec) -> Result<EncodedBlock> {
    if raw.is_empty() || raw.len() > MAX_BLOCK {
        return Err(Error::Limit);
    }
    let mut block = EncodedBlock {
        offset,
        codec: Codec::None,
        raw_len: raw.len() as u32,
        sha256: Sha256::digest(raw).into(),
        data: raw.to_vec(),
    };
    block.validate()?;
    if negotiated == Codec::Zlib {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(raw).map_err(|_| Error::Codec)?;
        let compressed = encoder.finish().map_err(|_| Error::Codec)?;
        if compressed.len() < raw.len() && compressed.len() <= MAX_ENCODED {
            block.codec = Codec::Zlib;
            block.data = compressed;
        }
    }
    Ok(block)
}

pub fn decode(block: &EncodedBlock, negotiated: Codec) -> Result<Vec<u8>> {
    block.validate()?;
    let raw = match block.codec {
        Codec::None => block.data.clone(),
        Codec::Zlib => {
            if negotiated != Codec::Zlib || block.data.len() < 2 || block.data[1] & 0x20 != 0 {
                return Err(Error::Codec);
            }
            // One extra byte detects output exceeding the declared bound. No partial
            // output escapes even if the decoder fills its buffer or rejects a frame.
            let mut raw = vec![0; block.raw_len as usize + 1];
            let mut decoder = Decompress::new(true);
            let status = decoder
                .decompress(&block.data, &mut raw, FlushDecompress::Finish)
                .map_err(|_| Error::Codec)?;
            if status != Status::StreamEnd
                || decoder.total_in() != block.data.len() as u64
                || decoder.total_out() != u64::from(block.raw_len)
            {
                return Err(Error::Codec);
            }
            raw.truncate(block.raw_len as usize);
            raw
        }
    };
    if <[u8; 32]>::from(Sha256::digest(&raw)) != block.sha256 {
        return Err(Error::Codec);
    }
    Ok(raw)
}

pub(crate) mod base64_bytes {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        decode_bytes(&encoded, crate::MAX_ENCODED).map_err(D::Error::custom)
    }

    pub(crate) fn decode_bytes(encoded: &str, max_bytes: usize) -> crate::Result<Vec<u8>> {
        // The envelope is bounded before serde; this checks encoded length before
        // allocating the decoded representation as well.
        if encoded.len() > 4 * max_bytes.div_ceil(3) {
            return Err(crate::Error::Limit);
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| crate::Error::Invalid("base64"))?;
        if bytes.len() > max_bytes || STANDARD.encode(&bytes) != encoded {
            return Err(crate::Error::Invalid("base64"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_and_fallback() {
        let binary: Vec<u8> = (0..MAX_BLOCK)
            .map(|n| (n.wrapping_mul(37) % 256) as u8)
            .collect();
        for raw in [
            &b"\0hello\n\xff"[..],
            "🌲".as_bytes(),
            &binary,
            &vec![b'a'; MAX_BLOCK],
        ] {
            for codec in [Codec::None, Codec::Zlib] {
                let block = encode(7, raw, codec).unwrap();
                assert!(decode(&block, codec).unwrap() == raw);
            }
        }
        assert_eq!(encode(0, b"x", Codec::Zlib).unwrap().codec, Codec::None);
    }
    #[test]
    fn incompressible_maximum_block_uses_none() {
        let mut state = 0x1234_5678_u32;
        let raw: Vec<u8> = (0..MAX_BLOCK)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let block = encode(0, &raw, Codec::Zlib).unwrap();
        assert_eq!(block.codec, Codec::None);
        assert!(decode(&block, Codec::Zlib).unwrap() == raw);
    }
    #[test]
    fn rejects_corruption_truncation_frames_and_dictionary() {
        let block = encode(0, &vec![0; MAX_BLOCK], Codec::Zlib).unwrap();
        assert!(decode(&block, Codec::None).is_err());
        for length in 0..block.data.len() {
            let mut damaged = block.clone();
            damaged.data.truncate(length);
            assert!(decode(&damaged, Codec::Zlib).is_err());
        }
        let mut damaged = block.clone();
        damaged.data.extend_from_slice(&block.data);
        assert!(decode(&damaged, Codec::Zlib).is_err());
        let mut damaged = block.clone();
        damaged.data.push(0);
        assert!(decode(&damaged, Codec::Zlib).is_err());
        let mut damaged = block.clone();
        damaged.data[1] |= 0x20;
        assert!(decode(&damaged, Codec::Zlib).is_err());
        let mut damaged = block.clone();
        damaged.sha256[0] ^= 1;
        assert!(decode(&damaged, Codec::Zlib).is_err());
        let mut damaged = block;
        damaged.raw_len = 1;
        assert!(decode(&damaged, Codec::Zlib).is_err());
    }
    #[test]
    fn rejects_nonbeneficial_zlib_representation() {
        let valid = encode(0, &[b'a'; 128], Codec::Zlib).unwrap();
        assert_eq!(valid.codec, Codec::Zlib);
        for raw_len in [valid.data.len() as u32, valid.data.len() as u32 - 1] {
            let mut block = valid.clone();
            block.raw_len = raw_len;
            assert!(matches!(block.validate(), Err(Error::Codec)));
            assert!(decode(&block, Codec::Zlib).is_err());
        }
    }
    #[test]
    fn rejects_bounds_and_length() {
        assert!(encode(0, &[], Codec::None).is_err());
        assert!(encode(0, &vec![0; MAX_BLOCK + 1], Codec::None).is_err());
        assert!(encode(MAX_OFFSET, b"a", Codec::None).is_err());
        let mut block = encode(0, b"abc", Codec::None).unwrap();
        block.raw_len = 2;
        assert!(decode(&block, Codec::None).is_err());
        block.raw_len = (MAX_BLOCK + 1) as u32;
        assert!(decode(&block, Codec::None).is_err());
    }
}
