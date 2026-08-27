//! Neither Cloudflare Workers Assets nor R2 compresses a generic binary type, so the producer encodes the
//! artifacts and every platform decodes them here.

use std::io::Read;

use brotli::enc::BrotliEncoderParams;

use crate::error::AppError;

pub const COMPRESSED_FILENAME_EXTENSION: &str = "br";

const BUFFER_BYTES: usize = 64 * 1024;

pub fn compress(plain_bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let parameters: BrotliEncoderParams = encoder_parameters();
    let mut compressed_bytes: Vec<u8> = Vec::new();

    brotli::BrotliCompress(&mut &plain_bytes[..], &mut compressed_bytes, &parameters)
        .map_err(|error| AppError::from(format!("brotli encode failed; [bytes={} error={error}]", plain_bytes.len())))?;

    Ok(compressed_bytes)
}

/// The highest quality and the largest window, affordable because only the producer encodes and it runs weekly.
fn encoder_parameters() -> BrotliEncoderParams {
    let mut parameters: BrotliEncoderParams = BrotliEncoderParams::default();
    parameters.quality = 11;
    parameters.lgwin = 22;

    parameters
}

pub fn decompress(compressed_bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut decompressed_bytes: Vec<u8> = Vec::new();

    brotli::Decompressor::new(compressed_bytes, BUFFER_BYTES)
        .read_to_end(&mut decompressed_bytes)
        .map_err(|error| {
            AppError::from(format!(
                "the bytes are not a brotli stream; [bytes={} error={error}]",
                compressed_bytes.len(),
            ))
        })?;

    Ok(decompressed_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::artifact::geometry::tests::one_feature_fgb_bytes;

    fn sample_shard_bytes() -> Vec<u8> {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite")).to_vec()
    }

    #[test]
    fn the_encoder_runs_at_the_highest_quality_and_the_largest_window() {
        let parameters: BrotliEncoderParams = encoder_parameters();

        assert_eq!(parameters.quality, 11);
        assert_eq!(parameters.lgwin, 22);
        /* A magic number would put bytes before the stream that `Decompressor` does not expect. */
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
