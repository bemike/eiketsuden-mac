//! Indexed images and PNG output.

use std::fmt;

/// An image of palette indices, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// A 16-colour palette, 8-bit RGB.
pub type Palette16 = [[u8; 3]; 16];

/// A PNG that could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngError(pub String);

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PNG encoding failed: {}", self.0)
    }
}

impl std::error::Error for PngError {}

/// Encode as an 8-bit indexed PNG with a 16-entry palette. Index 0 is fully transparent when
/// `transparent_zero` is set (sprites and chips), opaque otherwise.
pub fn encode_png(
    image: &IndexedImage,
    palette: &Palette16,
    transparent_zero: bool,
) -> Result<Vec<u8>, PngError> {
    if image.pixels.len() != image.width * image.height {
        return Err(PngError(format!(
            "{} pixels for a {}×{} image",
            image.pixels.len(),
            image.width,
            image.height
        )));
    }
    if let Some(&bad) = image.pixels.iter().find(|&&p| p >= 16) {
        return Err(PngError(format!(
            "colour index {bad} outside the 16-colour palette"
        )));
    }
    let width = u32::try_from(image.width).map_err(|e| PngError(e.to_string()))?;
    let height = u32::try_from(image.height).map_err(|e| PngError(e.to_string()))?;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(palette.iter().flatten().copied().collect::<Vec<u8>>());
        if transparent_zero {
            encoder.set_trns(vec![0u8]);
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| PngError(e.to_string()))?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|e| PngError(e.to_string()))?;
        writer.finish().map_err(|e| PngError(e.to_string()))?;
    }
    Ok(out)
}

/// A 16-step grey ramp, used when no palette could be located (reported as such).
pub fn grey_ramp() -> Palette16 {
    std::array::from_fn(|i| {
        let v = (i * 17) as u8;
        [v, v, v]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_with_transparency() {
        let image = IndexedImage {
            width: 3,
            height: 2,
            pixels: vec![0, 1, 15, 2, 0, 3],
        };
        let mut palette = grey_ramp();
        palette[1] = [255, 0, 0];
        let bytes = encode_png(&image, &palette, true).unwrap();

        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Indexed);
        assert_eq!(info.trns.as_deref(), Some(&[0u8][..]));
        let pal = info.palette.as_deref().unwrap().to_vec();
        assert_eq!(&pal[3..6], &[255, 0, 0]);
        let mut buf = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..frame.buffer_size()], &image.pixels[..]);
    }

    #[test]
    fn opaque_png_has_no_trns() {
        let image = IndexedImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        let bytes = encode_png(&image, &grey_ramp(), false).unwrap();
        let reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        assert!(reader.info().trns.is_none());
    }

    #[test]
    fn rejects_bad_images() {
        let bad = IndexedImage {
            width: 2,
            height: 2,
            pixels: vec![0; 3],
        };
        assert!(encode_png(&bad, &grey_ramp(), true).is_err());
        let bad = IndexedImage {
            width: 1,
            height: 1,
            pixels: vec![16],
        };
        assert!(encode_png(&bad, &grey_ramp(), true)
            .unwrap_err()
            .0
            .contains("outside"));
    }
}
