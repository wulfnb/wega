//! Matroska / WebM EBML demuxer and stream metadata extractor.

#[derive(Clone, Debug, PartialEq)]
pub struct WebmVideoTrack {
    pub track_number: u64,
    pub codec_id: String,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebmAudioTrack {
    pub track_number: u64,
    pub codec_id: String,
    pub channels: u32,
    pub sampling_frequency: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebmInfo {
    pub timecode_scale: u64, // Nanoseconds per tick (default 1,000,000)
    pub duration_seconds: f64,
    pub video_track: Option<WebmVideoTrack>,
    pub audio_track: Option<WebmAudioTrack>,
}

fn read_vint(bytes: &[u8], pos: usize, mask_id: bool) -> Result<(u64, usize), &'static str> {
    if pos >= bytes.len() {
        return Err("Unexpected end of data reading VINT");
    }
    let first = bytes[pos];
    if first == 0 {
        return Err("Invalid zero VINT marker");
    }
    let mut mask = 0x80u8;
    let mut length = 1usize;
    while (first & mask) == 0 {
        mask >>= 1;
        length += 1;
        if length > 8 {
            return Err("VINT length exceeds 8 bytes");
        }
    }
    if pos + length > bytes.len() {
        return Err("Unexpected end of data reading VINT payload");
    }
    let mut val = if mask_id {
        (first & !mask) as u64
    } else {
        first as u64
    };
    for i in 1..length {
        val = (val << 8) | (bytes[pos + i] as u64);
    }
    Ok((val, length))
}

fn read_uint(bytes: &[u8]) -> u64 {
    let mut val = 0u64;
    for &b in bytes {
        val = (val << 8) | (b as u64);
    }
    val
}

fn read_float(bytes: &[u8]) -> Option<f64> {
    if bytes.len() == 4 {
        let arr: [u8; 4] = bytes.try_into().ok()?;
        Some(f32::from_be_bytes(arr) as f64)
    } else if bytes.len() == 8 {
        let arr: [u8; 8] = bytes.try_into().ok()?;
        Some(f64::from_be_bytes(arr))
    } else {
        None
    }
}

impl WebmInfo {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let mut pos = 0;
        let (header_id, n) = read_vint(bytes, pos, false)?;
        pos += n;
        if header_id != 0x1A45DFA3 {
            return Err("Invalid EBML header identifier");
        }
        let (header_sz, n) = read_vint(bytes, pos, true)?;
        pos += n;
        pos += header_sz as usize; // Skip header elements

        let mut timecode_scale = 1_000_000u64;
        let mut duration_seconds = 0.0f64;
        let mut video_track: Option<WebmVideoTrack> = None;
        let mut audio_track: Option<WebmAudioTrack> = None;

        while pos < bytes.len() {
            let (elem_id, n) = read_vint(bytes, pos, false)?;
            pos += n;
            let (elem_sz, n) = read_vint(bytes, pos, true)?;
            pos += n;
            let elem_len = elem_sz as usize;
            let elem_end = pos + elem_len;
            if elem_end > bytes.len() {
                // If segment has unknown size or exceeds buffer, clamp to buffer
                // EBML unknown size is 0x01FFFFFFFFFFFFFF
            }

            if elem_id == 0x18538067 {
                // Segment
                let mut seg_pos = pos;
                let seg_end = if elem_sz == 0x01FFFFFFFFFFFFFF || elem_end > bytes.len() {
                    bytes.len()
                } else {
                    elem_end
                };

                while seg_pos < seg_end {
                    let (sub_id, n) = match read_vint(bytes, seg_pos, false) {
                        Ok(v) => v,
                        Err(_) => break,
                    };
                    seg_pos += n;
                    let (sub_sz, n) = match read_vint(bytes, seg_pos, true) {
                        Ok(v) => v,
                        Err(_) => break,
                    };
                    seg_pos += n;
                    let sub_end = seg_pos + (sub_sz as usize);

                    if sub_id == 0x1549A966 {
                        // Info
                        let mut inf_pos = seg_pos;
                        while inf_pos < sub_end && inf_pos < bytes.len() {
                            let (inf_id, n) = match read_vint(bytes, inf_pos, false) {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            inf_pos += n;
                            let (inf_sz, n) = match read_vint(bytes, inf_pos, true) {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            inf_pos += n;
                            let val_slice = &bytes[inf_pos..inf_pos + (inf_sz as usize)];
                            if inf_id == 0x2AD7B1 {
                                // TimecodeScale
                                timecode_scale = read_uint(val_slice);
                            } else if inf_id == 0x4489 {
                                // Duration
                                if let Some(d) = read_float(val_slice) {
                                    duration_seconds =
                                        (d * (timecode_scale as f64)) / 1_000_000_000.0;
                                }
                            }
                            inf_pos += inf_sz as usize;
                        }
                    } else if sub_id == 0x1654AE6B {
                        // Tracks
                        let mut tr_pos = seg_pos;
                        while tr_pos < sub_end && tr_pos < bytes.len() {
                            let (entry_id, n) = match read_vint(bytes, tr_pos, false) {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            tr_pos += n;
                            let (entry_sz, n) = match read_vint(bytes, tr_pos, true) {
                                Ok(v) => v,
                                Err(_) => break,
                            };
                            tr_pos += n;
                            let entry_end = tr_pos + (entry_sz as usize);

                            if entry_id == 0xAE {
                                // TrackEntry
                                let mut e_pos = tr_pos;
                                let mut tr_num = 1u64;
                                let mut tr_type = 1u64;
                                let mut codec_id = String::new();
                                let mut px_w = 0u32;
                                let mut px_h = 0u32;
                                let mut channels = 2u32;
                                let mut freq = 48000.0f64;

                                while e_pos < entry_end && e_pos < bytes.len() {
                                    let (prop_id, n) = match read_vint(bytes, e_pos, false) {
                                        Ok(v) => v,
                                        Err(_) => break,
                                    };
                                    e_pos += n;
                                    let (prop_sz, n) = match read_vint(bytes, e_pos, true) {
                                        Ok(v) => v,
                                        Err(_) => break,
                                    };
                                    e_pos += n;
                                    let prop_slice = &bytes[e_pos..e_pos + (prop_sz as usize)];

                                    if prop_id == 0xD7 {
                                        tr_num = read_uint(prop_slice);
                                    } else if prop_id == 0x83 {
                                        tr_type = read_uint(prop_slice);
                                    } else if prop_id == 0x86 {
                                        codec_id = String::from_utf8_lossy(prop_slice).to_string();
                                    } else if prop_id == 0xE0 {
                                        // Video settings
                                        let mut v_pos = e_pos;
                                        let v_end = e_pos + (prop_sz as usize);
                                        while v_pos < v_end && v_pos < bytes.len() {
                                            let (vid_id, n) = match read_vint(bytes, v_pos, false) {
                                                Ok(v) => v,
                                                Err(_) => break,
                                            };
                                            v_pos += n;
                                            let (vid_sz, n) = match read_vint(bytes, v_pos, true) {
                                                Ok(v) => v,
                                                Err(_) => break,
                                            };
                                            v_pos += n;
                                            let v_slice = &bytes[v_pos..v_pos + (vid_sz as usize)];
                                            if vid_id == 0xB0 {
                                                px_w = read_uint(v_slice) as u32;
                                            } else if vid_id == 0xBA {
                                                px_h = read_uint(v_slice) as u32;
                                            }
                                            v_pos += vid_sz as usize;
                                        }
                                    } else if prop_id == 0xE1 {
                                        // Audio settings
                                        let mut a_pos = e_pos;
                                        let a_end = e_pos + (prop_sz as usize);
                                        while a_pos < a_end && a_pos < bytes.len() {
                                            let (aud_id, n) = match read_vint(bytes, a_pos, false) {
                                                Ok(v) => v,
                                                Err(_) => break,
                                            };
                                            a_pos += n;
                                            let (aud_sz, n) = match read_vint(bytes, a_pos, true) {
                                                Ok(v) => v,
                                                Err(_) => break,
                                            };
                                            a_pos += n;
                                            let a_slice = &bytes[a_pos..a_pos + (aud_sz as usize)];
                                            if aud_id == 0x9F {
                                                channels = read_uint(a_slice) as u32;
                                            } else if aud_id == 0xB5 {
                                                if let Some(f) = read_float(a_slice) {
                                                    freq = f;
                                                }
                                            }
                                            a_pos += aud_sz as usize;
                                        }
                                    }
                                    e_pos += prop_sz as usize;
                                }

                                if tr_type == 1 {
                                    // Video track
                                    video_track = Some(WebmVideoTrack {
                                        track_number: tr_num,
                                        codec_id,
                                        pixel_width: px_w,
                                        pixel_height: px_h,
                                    });
                                } else if tr_type == 2 {
                                    // Audio track
                                    audio_track = Some(WebmAudioTrack {
                                        track_number: tr_num,
                                        codec_id,
                                        channels,
                                        sampling_frequency: freq,
                                    });
                                }
                            }
                            tr_pos = entry_end;
                        }
                    }
                    seg_pos = sub_end;
                }
                break;
            } else {
                pos = elem_end;
            }
        }

        Ok(Self {
            timecode_scale,
            duration_seconds,
            video_track,
            audio_track,
        })
    }
}
