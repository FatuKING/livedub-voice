//! PCM16 helpers for the translation socket: base64 encoding and fixed-size chunking.
//!
//! The socket carries PCM16 mono 24 kHz little-endian audio as base64 in both directions.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Sample rate of the audio sent to and received from the model.
pub const SAMPLE_RATE: u32 = 24_000;

/// Duration of each appended chunk. OpenAI recommends 200 ms: shorter chunks are buffered and
/// longer ones are split into 200 ms frames on the server.
pub const CHUNK_MS: u32 = 200;

/// Samples in one [`CHUNK_MS`] chunk (4800 at 24 kHz).
pub const CHUNK_SAMPLES: usize = samples_for_ms(CHUNK_MS);

/// Number of mono samples covering `ms` milliseconds at [`SAMPLE_RATE`].
pub const fn samples_for_ms(ms: u32) -> usize {
    (SAMPLE_RATE as usize * ms as usize) / 1000
}

/// Encodes samples as base64 PCM16 little-endian.
pub fn encode_pcm16(samples: &[i16]) -> String {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    STANDARD.encode(bytes)
}

/// Why a base64 audio delta could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("invalid base64 audio: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("PCM16 audio has an odd number of bytes ({0})")]
    OddLength(usize),
}

/// Decodes base64 PCM16 little-endian audio into samples.
pub fn decode_pcm16(encoded: &str) -> Result<Vec<i16>, DecodeError> {
    let bytes = STANDARD.decode(encoded)?;
    if bytes.len() % 2 != 0 {
        return Err(DecodeError::OddLength(bytes.len()));
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    Ok(pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
}

/// Collects audio of any buffer size and hands it out in fixed-size chunks.
///
/// Capture devices deliver buffers of whatever size they like; the socket works best with
/// [`CHUNK_SAMPLES`]-sample appends.
#[derive(Debug, Clone)]
pub struct Chunker {
    chunk_len: usize,
    pending: Vec<i16>,
}

impl Chunker {
    /// A chunker that yields chunks of `chunk_len` samples (at least 1).
    pub fn new(chunk_len: usize) -> Self {
        let chunk_len = chunk_len.max(1);
        Self {
            chunk_len,
            pending: Vec::with_capacity(chunk_len),
        }
    }

    /// Adds samples and returns every complete chunk now available, in order.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Vec<i16>> {
        self.pending.extend_from_slice(samples);
        let full = self.pending.len() / self.chunk_len * self.chunk_len;
        let chunks = self.pending[..full]
            .chunks_exact(self.chunk_len)
            .map(<[i16]>::to_vec)
            .collect();
        self.pending.drain(..full);
        chunks
    }

    /// Returns the leftover samples (shorter than a chunk), if any, and empties the buffer.
    pub fn flush(&mut self) -> Option<Vec<i16>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }

    /// Samples waiting for a complete chunk.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(CHUNK_SAMPLES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_constants_match_200_ms_at_24_khz() {
        assert_eq!(CHUNK_SAMPLES, 4800);
        assert_eq!(samples_for_ms(100), 2400);
        assert_eq!(samples_for_ms(1000), 24_000);
    }

    #[test]
    fn encodes_little_endian() {
        // 1 = 01 00, -1 = FF FF
        assert_eq!(encode_pcm16(&[1, -1]), "AQD//w==");
        assert_eq!(encode_pcm16(&[]), "");
    }

    #[test]
    fn base64_round_trip() {
        let samples: Vec<i16> = (0..9600)
            .map(|i| ((i * 37) % 65_536 - 32_768) as i16)
            .chain([i16::MIN, i16::MAX, 0, -1, 1])
            .collect();

        assert_eq!(decode_pcm16(&encode_pcm16(&samples)).unwrap(), samples);
    }

    #[test]
    fn decode_rejects_bad_input() {
        assert!(matches!(
            decode_pcm16("not base64!"),
            Err(DecodeError::Base64(_))
        ));
        // "AQD/" is 3 bytes.
        assert!(matches!(
            decode_pcm16("AQD/"),
            Err(DecodeError::OddLength(3))
        ));
    }

    #[test]
    fn chunker_splits_and_keeps_the_remainder() {
        let mut chunker = Chunker::new(4);

        assert!(chunker.push(&[1, 2, 3]).is_empty());
        assert_eq!(chunker.pending_len(), 3);
        assert_eq!(
            chunker.push(&[4, 5, 6, 7, 8, 9, 10]),
            vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]]
        );
        assert_eq!(chunker.flush(), Some(vec![9, 10]));
        assert_eq!(chunker.flush(), None);
    }

    #[test]
    fn chunker_preserves_every_sample_in_order() {
        let input: Vec<i16> = (0..10_000).map(|i| i as i16).collect();
        let mut chunker = Chunker::default();

        let mut output: Vec<i16> = Vec::new();
        for piece in input.chunks(441) {
            for chunk in chunker.push(piece) {
                assert_eq!(chunk.len(), CHUNK_SAMPLES);
                output.extend(chunk);
            }
        }
        output.extend(chunker.flush().unwrap_or_default());

        assert_eq!(output, input);
    }

    #[test]
    fn chunker_never_uses_zero_length_chunks() {
        let mut chunker = Chunker::new(0);

        assert_eq!(chunker.push(&[1, 2]), vec![vec![1], vec![2]]);
    }
}
