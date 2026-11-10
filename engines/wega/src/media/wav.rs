//! RIFF WAVE container parser and PCM audio sample extractor.

#[derive(Clone, Debug, PartialEq)]
pub struct WavInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub duration_seconds: f64,
    pub total_samples: usize,
    pub data_offset: usize,
    pub data_length: usize,
}

impl WavInfo {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 44 {
            return Err("WAV file too short for RIFF header");
        }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("Invalid RIFF/WAVE header");
        }
        let mut pos = 12;
        let mut fmt_found = false;
        let mut data_found = false;
        let mut channels = 1u16;
        let mut sample_rate = 44100u32;
        let mut bits_per_sample = 16u16;
        let mut data_offset = 0;
        let mut data_length = 0;

        while pos + 8 <= bytes.len() {
            let chunk_id = &bytes[pos..pos + 4];
            let chunk_size = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            pos += 8;

            if chunk_id == b"fmt " && chunk_size >= 16 {
                if pos + chunk_size > bytes.len() {
                    return Err("Malformed fmt chunk");
                }
                let audio_format = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                if audio_format != 1 && audio_format != 3 {
                    // 1 = PCM, 3 = IEEE float
                    return Err("Unsupported audio format in WAV");
                }
                channels = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
                sample_rate = u32::from_le_bytes([
                    bytes[pos + 4],
                    bytes[pos + 5],
                    bytes[pos + 6],
                    bytes[pos + 7],
                ]);
                bits_per_sample = u16::from_le_bytes([bytes[pos + 14], bytes[pos + 15]]);
                fmt_found = true;
            } else if chunk_id == b"data" {
                data_offset = pos;
                data_length = chunk_size.min(bytes.len() - pos);
                data_found = true;
            }
            pos += chunk_size;
            if chunk_size % 2 == 1 {
                pos += 1;
            }
        }

        if !fmt_found || !data_found || channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
            return Err("Missing required fmt or data chunks in WAV");
        }

        let bytes_per_sample = (bits_per_sample as usize) / 8;
        if bytes_per_sample == 0 {
            return Err("Invalid bits per sample");
        }
        let block_align = (channels as usize) * bytes_per_sample;
        let total_samples = data_length / block_align;
        let duration_seconds = total_samples as f64 / sample_rate as f64;

        Ok(Self {
            channels,
            sample_rate,
            bits_per_sample,
            duration_seconds,
            total_samples,
            data_offset,
            data_length,
        })
    }
}
