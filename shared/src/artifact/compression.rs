//! Brotli, applied by the producer and undone here, because neither CDN compresses a generic binary type for
//! us: a probe against Cloudflare Workers Assets returned a 1.5 MB FlatGeobuf whole with no `Content-Encoding`,
//! and R2 does not compress on its own either. Doing it in the producer and in `shared` is what covers iOS and
//! Android as well as the web, and any future destination.

use std::io::Read;

use brotli::enc::BrotliEncoderParams;

use crate::error::AppError;

/// Appended to an artifact's filename, so a published file's name states the form its bytes are in.
pub const COMPRESSED_FILENAME_EXTENSION: &str = "br";

/// The decoded length comes from the stream rather than from the manifest, so it is bounded before it is
/// allocated. Two orders of magnitude above the largest artifact this produces.
const DECOMPRESSED_CEILING_BYTES: usize = 512 * 1024 * 1024;

const BUFFER_BYTES: usize = 64 * 1024;

pub fn compress(plain_bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let parameters: BrotliEncoderParams = BrotliEncoderParams::default();
    let mut compressed_bytes: Vec<u8> = Vec::new();

    brotli::BrotliCompress(&mut &plain_bytes[..], &mut compressed_bytes, &parameters)
        .map_err(|error| AppError::from(format!("brotli encode failed; [bytes={} error={error}]", plain_bytes.len())))?;

    Ok(compressed_bytes)
}

pub fn decompress(compressed_bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut decompressed_bytes: Vec<u8> = Vec::new();
    let ceiling: u64 = DECOMPRESSED_CEILING_BYTES as u64 + 1;

    let read: usize = brotli::Decompressor::new(compressed_bytes, BUFFER_BYTES)
        .take(ceiling)
        .read_to_end(&mut decompressed_bytes)
        .map_err(|error| {
            AppError::from(format!(
                "the bytes are not a brotli stream; [bytes={} error={error}]",
                compressed_bytes.len(),
            ))
        })?;

    if read > DECOMPRESSED_CEILING_BYTES {
        return Err(AppError::from(format!(
            "a brotli stream decoded past the ceiling; [ceiling={DECOMPRESSED_CEILING_BYTES}]",
        )));
    }

    Ok(decompressed_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::artifact::geometry::tests::one_feature_fgb_bytes;

    fn sample_shard_bytes() -> Vec<u8> {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite")).to_vec()
    }

    /// The producer's encoder settings are the crate's defaults, so a change to them in a future release would
    /// silently alter every published artifact's size.
    #[test]
    fn the_encoder_runs_at_the_highest_quality_and_the_largest_window() {
        let parameters: BrotliEncoderParams = BrotliEncoderParams::default();

        assert_eq!(parameters.quality, 11);
        assert_eq!(parameters.lgwin, 22);
        assert!(!parameters.magic_number);
    }

    #[test]
    fn a_round_trip_returns_flatgeobuf_bytes_exactly() {
        let plain_bytes: Vec<u8> = one_feature_fgb_bytes();

        let restored_bytes: Vec<u8> = decompress(&compress(&plain_bytes).unwrap()).unwrap();

        assert_eq!(restored_bytes, plain_bytes);
    }

    #[test]
    fn a_round_trip_returns_shard_bytes_exactly() {
        let plain_bytes: Vec<u8> = sample_shard_bytes();

        let compressed_bytes: Vec<u8> = compress(&plain_bytes).unwrap();
        let restored_bytes: Vec<u8> = decompress(&compressed_bytes).unwrap();

        assert_eq!(restored_bytes, plain_bytes);
        assert!(compressed_bytes.len() < plain_bytes.len());
    }

    /// The case a producer bug would produce: bytes that match their digest but were never encoded.
    #[test]
    fn decompressing_plain_artifact_bytes_reports_the_wrong_form() {
        let error: AppError = decompress(&one_feature_fgb_bytes()).unwrap_err();

        assert!(error.to_string().contains("not a brotli stream"));
    }

    #[test]
    fn decompressing_a_truncated_stream_reports_the_wrong_form() {
        let compressed_bytes: Vec<u8> = compress(&sample_shard_bytes()).unwrap();
        let truncated_bytes: &[u8] = &compressed_bytes[..compressed_bytes.len() / 2];

        assert!(decompress(truncated_bytes).is_err());
    }

    #[test]
    fn a_round_trip_returns_an_empty_input_exactly() {
        assert_eq!(decompress(&compress(&[]).unwrap()).unwrap(), Vec::<u8>::new());
    }
}
