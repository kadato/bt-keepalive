//! Minimal stereo WAV writer for `--dry-run` and tests.
//!
//! Writes 16-bit PCM WAV so `--render-wav out.wav` produces a file
//! every player and stdlib opens. No dependency; the header is 44 bytes.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Write interleaved stereo `f32` samples at `sample_rate` Hz as 16-bit PCM.
pub fn write_wav_stereo_16(path: &Path, samples: &[f32], sample_rate: u32) -> io::Result<()> {
    if !samples.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stereo samples must have even length",
        ));
    }
    let sr = sample_rate.max(1);
    let data_bytes = u32::try_from(samples.len() * 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many samples for WAV"))?;

    let mut f = File::create(path)?;
    // RIFF header.
    f.write_all(b"RIFF")?;
    f.write_all(&(36u32.wrapping_add(data_bytes)).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    // fmt chunk: PCM integer (tag 1), 2 channels, 16 bits.
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 2 * 2).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    // data chunk.
    f.write_all(b"data")?;
    f.write_all(&data_bytes.to_le_bytes())?;
    for s in samples {
        let pcm = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        f.write_all(&pcm.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_header_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.wav");
        let samples = [0.0f32, 0.0, 0.5, -0.5];
        write_wav_stereo_16(&path, &samples, 48_000).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            48_000
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        assert_eq!(&bytes[36..40], b"data");
        // 0.5 scales to 16384; samples start at byte 44, 2 bytes each.
        let third = i16::from_le_bytes([bytes[48], bytes[49]]);
        assert_eq!(third, 16384);
        let fourth = i16::from_le_bytes([bytes[50], bytes[51]]);
        assert_eq!(fourth, -16384);
    }

    #[test]
    fn clips_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        write_wav_stereo_16(&path, &[2.0, -2.0], 48_000).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), 32767);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), -32767);
    }

    #[test]
    fn odd_length_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.wav");
        assert!(write_wav_stereo_16(&path, &[0.1, 0.2, 0.3], 48_000).is_err());
    }
}
