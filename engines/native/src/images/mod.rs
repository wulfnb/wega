//! Owned decoded image payloads and bounded decoding of authorized bytes.
//!
//! Extracted from the production decoder in the patched reference document.
//! No fetch, document identity, DOM, CSP decision, path resolver or fallback
//! network request belongs here. All callers share one process admission budget.
mod budget;
mod svg;
use std::sync::Arc;
pub use svg::{SvgImageData, SvgIntrinsicDimensions};

#[derive(Debug, Clone, PartialEq)]
pub struct RasterImageData {
    pub width: u32,
    pub height: u32,
    pub data: peniko::Blob<u8>,
}
impl RasterImageData {
    pub fn new(width: u32, height: u32, data: Arc<Vec<u8>>) -> Self {
        Self {
            width,
            height,
            data: peniko::Blob::new(data),
        }
    }
}

/// An admitted pixel allocation. Moving its Arc to a node/cache/display list
/// retains the exact owner charged by the process budget, without a pixel copy.
pub struct DecodedRaster {
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>,
}

#[derive(Debug)]
pub enum RasterDecodeError {
    /// Admission/dimension/allocation rejection: do not try another decoder.
    Rejected(String),
    /// Raster probing/decoding failed; the existing caller may try bounded SVG.
    Format(String),
}

/// Decode exactly the existing production raster formats and retain the existing
/// 8192-side, 8 Mi-pixel, 64 MiB allocation and process admission limits.
pub fn decode_raster(bytes: &[u8]) -> Result<DecodedRaster, RasterDecodeError> {
    use RasterDecodeError::{Format, Rejected};
    let permit = budget::DecodePermit::reserve().map_err(Rejected)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Rejected(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|e| Format(e.to_string()))?;
    use image::ImageDecoder;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 8 * 1024 * 1024
        || decoder.total_bytes() > 64 * 1024 * 1024
    {
        return Err(Rejected(
            "Decoded image exceeds dimensions/allocation budget".into(),
        ));
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(|e| Format(e.to_string()))?;
    let data = Arc::new(image.into_rgba8().into_raw());
    permit.retain(&data, data.len()).map_err(Rejected)?;
    Ok(DecodedRaster {
        width,
        height,
        data,
    })
}
