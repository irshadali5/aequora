//! Checksummed framing and serialization codecs.

use aequora_types::ProtocolVersion;
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Frame flag indicating a zstd-compressed Postcard payload.
pub const FLAG_ZSTD: u8 = 0b0000_0001;
const KNOWN_FLAGS: u8 = FLAG_ZSTD;

/// Optional payload compression selected after capability negotiation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Compression {
    /// Send the serialized payload directly.
    #[default]
    None,
    /// Use zstd when the payload reaches the configured threshold and compression is smaller.
    Zstd { level: i32 },
}

/// Encoding controls for one frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodeOptions {
    /// Negotiated compression algorithm.
    pub compression: Compression,
    /// Do not spend CPU compressing payloads smaller than this value.
    pub compression_threshold: usize,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            compression: Compression::None,
            compression_threshold: 4_096,
        }
    }
}

/// Independent wire and decompressed limits used to prevent compression bombs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    /// Maximum bytes carried by the frame.
    pub max_wire_bytes: usize,
    /// Maximum bytes permitted after decompression.
    pub max_decompressed_bytes: usize,
}

/// Aequora protocol magic.
pub const MAGIC: [u8; 4] = *b"AEQ1";
/// Fixed header size: magic, version, flags, kind, length, and BLAKE3 digest.
pub const HEADER_LEN: usize = 44;

/// Frame message discriminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MessageKind {
    /// A synchronization request.
    SyncRequest = 1,
    /// A synchronization response.
    SyncResponse = 2,
    /// A bootstrap request.
    BootstrapRequest = 3,
    /// A bootstrap response.
    BootstrapResponse = 4,
    /// A request for a sequence of bounded bootstrap pages.
    SnapshotStreamRequest = 5,
    /// One page in a bounded bootstrap stream.
    SnapshotStreamResponse = 6,
    /// An advisory journal-advance notification.
    PushHint = 7,
    /// A transport-level error encoded without domain payloads.
    TransportError = 8,
}

impl TryFrom<u8> for MessageKind {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::SyncRequest),
            2 => Ok(Self::SyncResponse),
            3 => Ok(Self::BootstrapRequest),
            4 => Ok(Self::BootstrapResponse),
            5 => Ok(Self::SnapshotStreamRequest),
            6 => Ok(Self::SnapshotStreamResponse),
            7 => Ok(Self::PushHint),
            8 => Ok(Self::TransportError),
            other => Err(CodecError::MessageKind(other)),
        }
    }
}

/// Decoded, integrity-checked protocol frame.
#[derive(Clone, Copy, Debug)]
pub struct DecodedFrame<'a> {
    /// Protocol version from the frame header.
    pub protocol: ProtocolVersion,
    /// Reserved feature flags.
    pub flags: u8,
    /// Payload message kind.
    pub kind: MessageKind,
    /// Verified serialized payload.
    pub payload: &'a [u8],
}

/// Wire decoding or encoding failure.
#[derive(Debug, Error)]
pub enum CodecError {
    /// Frame is shorter than the fixed header.
    #[error("frame is shorter than the {HEADER_LEN}-byte header")]
    Truncated,
    /// Magic bytes do not identify Aequora.
    #[error("invalid protocol magic")]
    Magic,
    /// Message kind is not supported.
    #[error("unsupported message kind {0}")]
    MessageKind(u8),
    /// Declared payload length does not match the frame.
    #[error("declared payload length does not match the frame")]
    Length,
    /// Payload exceeded the configured limit.
    #[error("payload length {actual} exceeds limit {maximum}")]
    PayloadTooLarge { actual: usize, maximum: usize },
    /// Payload digest did not match.
    #[error("payload integrity check failed")]
    Integrity,
    /// Frame uses flag bits unknown to this implementation.
    #[error("frame contains unsupported flags {0:#010b}")]
    Flags(u8),
    /// Peer requested compression that was not compiled into this crate.
    #[error("zstd compression support is not enabled")]
    CompressionUnavailable,
    /// Compression or decompression failed.
    #[error("zstd processing failed: {0}")]
    Compression(String),
    /// Postcard serialization failed.
    #[error("postcard serialization failed: {0}")]
    Postcard(#[from] postcard::Error),
    /// RON diagnostic serialization failed.
    #[cfg(feature = "ron")]
    #[error("RON serialization failed: {0}")]
    Ron(String),
    /// Optional JSON diagnostic serialization or deserialization failed.
    #[cfg(feature = "json")]
    #[error("JSON serialization failed: {0}")]
    Json(String),
}

/// Encodes a serializable value as a checksummed Postcard Aequora frame.
///
/// # Errors
///
/// Returns [`CodecError`] when Postcard serialization fails or the payload cannot fit in
/// the frame's 32-bit length field.
pub fn encode<T: Serialize>(
    protocol: ProtocolVersion,
    kind: MessageKind,
    value: &T,
) -> Result<Vec<u8>, CodecError> {
    encode_with_options(protocol, kind, value, EncodeOptions::default())
}

/// Encodes a checksummed frame with thresholded negotiated compression.
///
/// # Errors
///
/// Returns [`CodecError`] for serialization, compression, or length failures.
pub fn encode_with_options<T: Serialize>(
    protocol: ProtocolVersion,
    kind: MessageKind,
    value: &T,
    options: EncodeOptions,
) -> Result<Vec<u8>, CodecError> {
    let serialized = postcard::to_stdvec(value)?;
    let (flags, payload) = compress_payload(serialized, options)?;
    let length = u32::try_from(payload.len()).map_err(|_| CodecError::PayloadTooLarge {
        actual: payload.len(),
        maximum: u32::MAX as usize,
    })?;
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&MAGIC);
    frame.extend_from_slice(&protocol.0.to_be_bytes());
    frame.push(flags);
    frame.push(kind as u8);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(blake3::hash(&payload).as_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Validates and splits a frame without deserializing its payload.
///
/// # Errors
///
/// Returns [`CodecError`] for malformed headers, invalid magic or kinds, excessive sizes,
/// inconsistent lengths, or a payload digest mismatch.
pub fn decode_frame(frame: &[u8], max_payload: usize) -> Result<DecodedFrame<'_>, CodecError> {
    if frame.len() < HEADER_LEN {
        return Err(CodecError::Truncated);
    }
    if frame[..4] != MAGIC {
        return Err(CodecError::Magic);
    }
    let protocol = ProtocolVersion(u16::from_be_bytes([frame[4], frame[5]]));
    let flags = frame[6];
    if flags & !KNOWN_FLAGS != 0 {
        return Err(CodecError::Flags(flags));
    }
    let kind = MessageKind::try_from(frame[7])?;
    let declared = u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    if declared > max_payload {
        return Err(CodecError::PayloadTooLarge {
            actual: declared,
            maximum: max_payload,
        });
    }
    if frame.len() != HEADER_LEN + declared {
        return Err(CodecError::Length);
    }
    let payload = &frame[HEADER_LEN..];
    let expected = &frame[12..HEADER_LEN];
    if blake3::hash(payload).as_bytes() != expected {
        return Err(CodecError::Integrity);
    }
    Ok(DecodedFrame {
        protocol,
        flags,
        kind,
        payload,
    })
}

/// Decodes and deserializes a framed Postcard value of the expected kind.
///
/// # Errors
///
/// Returns [`CodecError`] when frame validation or Postcard deserialization fails.
pub fn decode<T: DeserializeOwned>(
    frame: &[u8],
    expected_kind: MessageKind,
    max_payload: usize,
) -> Result<(ProtocolVersion, T), CodecError> {
    decode_with_limits(
        frame,
        expected_kind,
        DecodeLimits {
            max_wire_bytes: max_payload,
            max_decompressed_bytes: max_payload,
        },
    )
}

/// Decodes a frame with separate compressed-wire and decompressed payload limits.
///
/// # Errors
///
/// Returns [`CodecError`] for framing, integrity, decompression, size, or Postcard failures.
pub fn decode_with_limits<T: DeserializeOwned>(
    frame: &[u8],
    expected_kind: MessageKind,
    limits: DecodeLimits,
) -> Result<(ProtocolVersion, T), CodecError> {
    let decoded = decode_frame(frame, limits.max_wire_bytes)?;
    if decoded.kind != expected_kind {
        return Err(CodecError::MessageKind(decoded.kind as u8));
    }
    let decompressed;
    let payload = if decoded.flags & FLAG_ZSTD != 0 {
        decompressed = decompress_payload(decoded.payload, limits.max_decompressed_bytes)?;
        decompressed.as_slice()
    } else {
        if decoded.payload.len() > limits.max_decompressed_bytes {
            return Err(CodecError::PayloadTooLarge {
                actual: decoded.payload.len(),
                maximum: limits.max_decompressed_bytes,
            });
        }
        decoded.payload
    };
    let value = postcard::from_bytes(payload)?;
    Ok((decoded.protocol, value))
}

fn compress_payload(payload: Vec<u8>, options: EncodeOptions) -> Result<(u8, Vec<u8>), CodecError> {
    match options.compression {
        Compression::None => Ok((0, payload)),
        Compression::Zstd { level: _ } if payload.len() < options.compression_threshold => {
            Ok((0, payload))
        }
        #[cfg(feature = "zstd")]
        Compression::Zstd { level } => {
            let compressed = zstd::stream::encode_all(payload.as_slice(), level)
                .map_err(|error| CodecError::Compression(error.to_string()))?;
            if compressed.len() < payload.len() {
                Ok((FLAG_ZSTD, compressed))
            } else {
                Ok((0, payload))
            }
        }
        #[cfg(not(feature = "zstd"))]
        Compression::Zstd { .. } => Err(CodecError::CompressionUnavailable),
    }
}

#[cfg(feature = "zstd")]
fn decompress_payload(payload: &[u8], maximum: usize) -> Result<Vec<u8>, CodecError> {
    use std::io::Read;
    let decoder = zstd::stream::read::Decoder::new(payload)
        .map_err(|error| CodecError::Compression(error.to_string()))?;
    let take_limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bounded = decoder.take(take_limit);
    let mut output = Vec::new();
    bounded
        .read_to_end(&mut output)
        .map_err(|error| CodecError::Compression(error.to_string()))?;
    if output.len() > maximum {
        return Err(CodecError::PayloadTooLarge {
            actual: output.len(),
            maximum,
        });
    }
    Ok(output)
}

#[cfg(not(feature = "zstd"))]
fn decompress_payload(_payload: &[u8], _maximum: usize) -> Result<Vec<u8>, CodecError> {
    Err(CodecError::CompressionUnavailable)
}

/// Serializes a value for human-readable diagnostics and fixtures.
///
/// # Errors
///
/// Returns [`CodecError`] when the value cannot be represented as RON.
#[cfg(feature = "ron")]
pub fn to_ron<T: Serialize>(value: &T) -> Result<String, CodecError> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
        .map_err(|error| CodecError::Ron(error.to_string()))
}

/// Serializes a value as optional human-readable JSON for debugging, administration, or web
/// interoperability. JSON is never used by the primary synchronization transport.
///
/// # Errors
///
/// Returns [`CodecError`] when the value cannot be represented as JSON.
#[cfg(feature = "json")]
pub fn to_json<T: Serialize>(value: &T) -> Result<String, CodecError> {
    serde_json::to_string(value).map_err(|error| CodecError::Json(error.to_string()))
}

/// Deserializes optional diagnostic JSON into a caller-selected DTO.
///
/// # Errors
///
/// Returns [`CodecError`] when the JSON is malformed or does not match the DTO.
#[cfg(feature = "json")]
pub fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, CodecError> {
    serde_json::from_str(value).map_err(|error| CodecError::Json(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Example {
        value: u32,
    }

    #[test]
    fn framed_values_round_trip() -> Result<(), CodecError> {
        let frame = encode(
            ProtocolVersion::V1,
            MessageKind::SyncRequest,
            &Example { value: 42 },
        )?;
        let (version, decoded) = decode::<Example>(&frame, MessageKind::SyncRequest, 1_024)?;
        assert_eq!(version, ProtocolVersion::V1);
        assert_eq!(decoded, Example { value: 42 });
        Ok(())
    }

    #[test]
    fn payload_tampering_is_detected() -> Result<(), CodecError> {
        let mut frame = encode(
            ProtocolVersion::V1,
            MessageKind::SyncRequest,
            &Example { value: 1 },
        )?;
        let last = frame.len().saturating_sub(1);
        frame[last] ^= 1;
        assert!(matches!(
            decode_frame(&frame, 1_024),
            Err(CodecError::Integrity)
        ));
        Ok(())
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn compressed_payloads_round_trip_with_a_decompressed_limit() -> Result<(), CodecError> {
        let value = vec![42_u8; 32_768];
        let frame = encode_with_options(
            ProtocolVersion::V1,
            MessageKind::SyncResponse,
            &value,
            EncodeOptions {
                compression: Compression::Zstd { level: 1 },
                compression_threshold: 1,
            },
        )?;
        assert_ne!(decode_frame(&frame, 1_024)?.flags & FLAG_ZSTD, 0);
        let (_, decoded) = decode_with_limits::<Vec<u8>>(
            &frame,
            MessageKind::SyncResponse,
            DecodeLimits {
                max_wire_bytes: 1_024,
                max_decompressed_bytes: 64 * 1_024,
            },
        )?;
        assert_eq!(decoded, value);
        assert!(matches!(
            decode_with_limits::<Vec<u8>>(
                &frame,
                MessageKind::SyncResponse,
                DecodeLimits {
                    max_wire_bytes: 1_024,
                    max_decompressed_bytes: 1_024
                },
            ),
            Err(CodecError::PayloadTooLarge { .. })
        ));
        Ok(())
    }

    #[cfg(feature = "json")]
    #[test]
    fn optional_json_round_trips_without_changing_wire_framing() -> Result<(), CodecError> {
        let encoded = to_json(&Example { value: 42 })?;
        assert_eq!(from_json::<Example>(&encoded)?, Example { value: 42 });
        Ok(())
    }
}
