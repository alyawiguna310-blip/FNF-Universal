//! Decoded texture resources and PNG encoding/decoding.
//!
//! A [`TextureAsset`] is CPU-side RGBA8 data that the renderer turns into a GPU
//! texture. Decoding lives here so that asset loading stays independent of any
//! rendering backend, and so the same bytes can be used by tests without a GPU.

use std::error::Error;
use std::fmt;
use std::io::Cursor;

use fu_vfs::VfsPath;

use crate::asset::Asset;

/// Why a texture could not be decoded.
#[derive(Debug)]
pub struct TextureDecodeError {
    message: String,
}

impl TextureDecodeError {
    /// Creates an error with a human readable explanation.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TextureDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TextureDecodeError {}

/// A decoded image: straight (non-premultiplied) RGBA8 pixels, row-major.
#[derive(Clone, PartialEq)]
pub struct TextureAsset {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA8 data.
    pub rgba8: Vec<u8>,
}

impl fmt::Debug for TextureAsset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextureAsset")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba8.len())
            .finish()
    }
}

impl TextureAsset {
    /// Wraps RGBA8 pixels after validating the dimensions.
    pub fn from_rgba8(width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self, TextureDecodeError> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4));
        if width == 0 || height == 0 {
            return Err(TextureDecodeError::new(
                "texture dimensions must be non-zero",
            ));
        }
        if expected != Some(rgba8.len()) {
            return Err(TextureDecodeError::new(format!(
                "texture data is {} bytes, expected {} for {width}x{height} RGBA8",
                rgba8.len(),
                expected.unwrap_or(0)
            )));
        }
        Ok(Self {
            width,
            height,
            rgba8,
        })
    }

    /// A fully transparent texture of the given size.
    pub fn transparent(width: u32, height: u32) -> Self {
        let pixels = (width as usize) * (height as usize);
        Self {
            width,
            height,
            rgba8: vec![0; pixels * 4],
        }
    }

    /// A single flat colour texture, used for missing-resource fallbacks.
    pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut texture = Self::transparent(width, height);
        for pixel in texture.rgba8.as_chunks_mut::<4>().0 {
            *pixel = color;
        }
        texture
    }

    /// The RGBA value at `(x, y)`, or `None` when outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y as usize) * (self.width as usize) + x as usize) * 4;
        Some([
            self.rgba8[index],
            self.rgba8[index + 1],
            self.rgba8[index + 2],
            self.rgba8[index + 3],
        ])
    }

    /// Decodes 8-bit PNG data, expanding RGB, grayscale, alpha and indexed
    /// variants to RGBA8.
    ///
    /// Indexed sheets are common in game art, so the decoder asks libpng to
    /// expand palettes and transparency chunks instead of rejecting them. Only
    /// 8-bit output is supported; 16-bit PNGs are rejected with a clear message
    /// rather than being silently truncated.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, TextureDecodeError> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder
            .read_info()
            .map_err(|error| TextureDecodeError::new(format!("PNG header: {error}")))?;
        let size = reader.output_buffer_size().ok_or_else(|| {
            TextureDecodeError::new("PNG image is too large to decode into memory")
        })?;
        let mut buffer = vec![0u8; size];
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|error| TextureDecodeError::new(format!("PNG data: {error}")))?;
        if info.bit_depth != png::BitDepth::Eight {
            return Err(TextureDecodeError::new(format!(
                "only 8-bit PNGs are supported, found {:?}",
                info.bit_depth
            )));
        }
        let rgba8 = expand_to_rgba(info.color_type, info.width, info.height, &buffer)?;
        Self::from_rgba8(info.width, info.height, rgba8)
    }

    /// Encodes this texture as an 8-bit RGBA PNG.
    ///
    /// Used to generate and persist the procedural default pack.
    pub fn encode_png(&self) -> Result<Vec<u8>, TextureDecodeError> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|error| TextureDecodeError::new(format!("PNG encode header: {error}")))?;
            writer
                .write_image_data(&self.rgba8)
                .map_err(|error| TextureDecodeError::new(format!("PNG encode data: {error}")))?;
        }
        Ok(bytes)
    }

    /// Copies a sub-rectangle out of this texture, clamping to its bounds.
    ///
    /// Atlas frames are cut with this; pixels outside the source become transparent.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Self {
        let mut out = Self::transparent(width, height);
        for row in 0..height {
            for column in 0..width {
                if let Some(pixel) = self.pixel(x + column, y + row) {
                    let index = ((row as usize) * (width as usize) + column as usize) * 4;
                    out.rgba8[index..index + 4].copy_from_slice(&pixel);
                }
            }
        }
        out
    }
}

fn expand_to_rgba(
    color_type: png::ColorType,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> Result<Vec<u8>, TextureDecodeError> {
    let pixels = (width as usize) * (height as usize);
    let channels = match color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        other => {
            return Err(TextureDecodeError::new(format!(
                "unsupported PNG colour type {other:?}"
            )))
        }
    };
    let needed = pixels * channels;
    if bytes.len() < needed {
        return Err(TextureDecodeError::new(format!(
            "PNG produced {} bytes, needed {needed}",
            bytes.len()
        )));
    }

    let mut rgba = Vec::with_capacity(pixels * 4);
    for pixel in 0..pixels {
        let source = &bytes[pixel * channels..pixel * channels + channels];
        match channels {
            4 => rgba.extend_from_slice(source),
            3 => rgba.extend_from_slice(&[source[0], source[1], source[2], 255]),
            2 => rgba.extend_from_slice(&[source[0], source[0], source[0], source[1]]),
            _ => rgba.extend_from_slice(&[source[0], source[0], source[0], 255]),
        }
    }
    Ok(rgba)
}

impl Asset for TextureAsset {
    type Error = TextureDecodeError;

    fn decode(bytes: &[u8], path: &VfsPath) -> Result<Self, Self::Error> {
        Self::decode_png(bytes).map_err(|error| TextureDecodeError::new(format!("{path}: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TextureAsset {
        let mut texture = TextureAsset::transparent(2, 2);
        texture.rgba8[0..4].copy_from_slice(&[255, 0, 0, 255]);
        texture.rgba8[4..8].copy_from_slice(&[0, 255, 0, 255]);
        texture.rgba8[8..12].copy_from_slice(&[0, 0, 255, 128]);
        texture
    }

    #[test]
    fn png_round_trip_preserves_pixels() {
        let texture = sample();
        let png = texture.encode_png().expect("encode");
        let decoded = TextureAsset::decode_png(&png).expect("decode");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded, texture);
        assert_eq!(decoded.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(decoded.pixel(1, 0), Some([0, 255, 0, 255]));
        assert_eq!(decoded.pixel(0, 1), Some([0, 0, 255, 128]));
        assert_eq!(decoded.pixel(1, 1), Some([0, 0, 0, 0]));
        assert_eq!(decoded.pixel(2, 0), None);
    }

    #[test]
    fn invalid_data_is_rejected_with_a_message() {
        let error = TextureAsset::decode_png(b"not a png").unwrap_err();
        assert!(error.to_string().contains("PNG"), "got: {error}");

        let error = TextureAsset::from_rgba8(2, 2, vec![0; 15]).unwrap_err();
        assert!(error.to_string().contains("expected 16"), "got: {error}");
        assert!(TextureAsset::from_rgba8(0, 2, vec![]).is_err());
    }

    #[test]
    fn crop_clamps_and_fills_outside_pixels_transparently() {
        let texture = sample();
        let cropped = texture.crop(0, 1, 2, 2);
        assert_eq!(cropped.pixel(0, 0), Some([0, 0, 255, 128]));
        assert_eq!(cropped.pixel(1, 0), Some([0, 0, 0, 0]));
        assert_eq!(cropped.pixel(1, 1), Some([0, 0, 0, 0]));
    }

    #[test]
    fn rgb_png_is_expanded_to_opaque_rgba() {
        // 1x1 RGB PNG, produced by the encoder with the RGB colour type.
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[10, 20, 30]).expect("data");
        }
        let decoded = TextureAsset::decode_png(&bytes).expect("decode");
        assert_eq!(decoded.pixel(0, 0), Some([10, 20, 30, 255]));
    }
}
