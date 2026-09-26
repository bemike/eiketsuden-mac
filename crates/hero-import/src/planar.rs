//! 4 bpp planar graphics of the DOS/V builds.
//!
//! An image `w` pixels wide (a multiple of 8) and `h` high is stored as 4 bit-planes one after
//! the other, each `h` rows of `w / 8` bytes, most significant bit = leftmost pixel. The sprite
//! and map-chip archives hold **cells** of 16×16 pixels = 4 planes × 32 bytes = 128 bytes.
//!
//! Plane `p` is bit `p` of the 4-bit colour index. This was checked visually on the Korean
//! DOS/V files: with this order and the `MAIN.EXE` palettes, unit sprites show skin tones,
//! steel and outlines where they belong (any other plane order scrambles the colours). Colour
//! index 0 (all planes clear) is transparent: the engine builds its blit mask as the NOT of
//! the OR of the planes.
//!
//! A second arrangement, **packed** planar, stores every group of 8 pixels as 4 consecutive
//! bytes (plane 0, 1, 2, 3), groups left to right, rows top to bottom. `HEXGRP.R3` entry 0 and
//! the raw pictures inside `OPGRP.R3` / `END*GRP.R3` use it.

use crate::image::IndexedImage;
use std::fmt;

/// Width and height of a cell in pixels.
pub const CELL_PX: usize = 16;
/// Bytes of one cell (4 planes × 16 rows × 2 bytes).
pub const CELL_BYTES: usize = 128;

/// A planar buffer whose size does not match the requested geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanarError {
    /// The width is not a positive multiple of 8, or the height is 0.
    BadGeometry { width: usize, height: usize },
    /// The buffer length is not what the geometry requires.
    LengthMismatch { len: usize, expected: usize },
    /// The buffer is not a whole, non-zero number of 128-byte cells.
    NotCells { len: usize },
}

impl fmt::Display for PlanarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanarError::BadGeometry { width, height } => {
                write!(
                    f,
                    "planar image {width}×{height}: width must be a multiple of 8"
                )
            }
            PlanarError::LengthMismatch { len, expected } => write!(
                f,
                "planar data has {len} bytes, the geometry needs {expected}"
            ),
            PlanarError::NotCells { len } => {
                write!(f, "{len} bytes are not a whole number of 128-byte cells")
            }
        }
    }
}

impl std::error::Error for PlanarError {}

/// Bytes needed for a `width × height` 4 bpp planar image.
pub fn planar_len(width: usize, height: usize) -> usize {
    width / 8 * height * 4
}

/// Decode a planar image into one colour index (0–15) per pixel, row-major.
pub fn decode(data: &[u8], width: usize, height: usize) -> Result<IndexedImage, PlanarError> {
    if width == 0 || width % 8 != 0 || height == 0 {
        return Err(PlanarError::BadGeometry { width, height });
    }
    let expected = planar_len(width, height);
    if data.len() != expected {
        return Err(PlanarError::LengthMismatch {
            len: data.len(),
            expected,
        });
    }
    let stride = width / 8;
    let plane_len = stride * height;
    let mut pixels = vec![0u8; width * height];
    for (i, px) in pixels.iter_mut().enumerate() {
        let (y, x) = (i / width, i % width);
        let byte = y * stride + x / 8;
        let mask = 0x80 >> (x % 8);
        for plane in 0..4 {
            if data[plane * plane_len + byte] & mask != 0 {
                *px |= 1 << plane;
            }
        }
    }
    Ok(IndexedImage {
        width,
        height,
        pixels,
    })
}

/// Encode colour indices (0–15, row-major) as planar data; the inverse of [`decode`]. Used to
/// build synthetic fixtures.
pub fn encode(image: &IndexedImage) -> Result<Vec<u8>, PlanarError> {
    let (width, height) = (image.width, image.height);
    if width == 0 || width % 8 != 0 || height == 0 {
        return Err(PlanarError::BadGeometry { width, height });
    }
    if image.pixels.len() != width * height {
        return Err(PlanarError::LengthMismatch {
            len: image.pixels.len(),
            expected: width * height,
        });
    }
    let stride = width / 8;
    let plane_len = stride * height;
    let mut out = vec![0u8; plane_len * 4];
    for (i, &px) in image.pixels.iter().enumerate() {
        let (y, x) = (i / width, i % width);
        let byte = y * stride + x / 8;
        for plane in 0..4 {
            if px & (1 << plane) != 0 {
                out[plane * plane_len + byte] |= 0x80 >> (x % 8);
            }
        }
    }
    Ok(out)
}

fn check_geometry(width: usize, height: usize, len: usize) -> Result<(), PlanarError> {
    if width == 0 || width % 8 != 0 || height == 0 {
        return Err(PlanarError::BadGeometry { width, height });
    }
    let expected = planar_len(width, height);
    if len != expected {
        return Err(PlanarError::LengthMismatch { len, expected });
    }
    Ok(())
}

/// Decode a **packed** planar image (8 pixels = 4 consecutive plane bytes) into colour indices.
pub fn decode_packed(
    data: &[u8],
    width: usize,
    height: usize,
) -> Result<IndexedImage, PlanarError> {
    check_geometry(width, height, data.len())?;
    let mut pixels = vec![0u8; width * height];
    for (i, px) in pixels.iter_mut().enumerate() {
        let (y, x) = (i / width, i % width);
        let group = (y * (width / 8) + x / 8) * 4;
        let mask = 0x80 >> (x % 8);
        for plane in 0..4 {
            if data[group + plane] & mask != 0 {
                *px |= 1 << plane;
            }
        }
    }
    Ok(IndexedImage {
        width,
        height,
        pixels,
    })
}

/// Encode colour indices as packed planar data; the inverse of [`decode_packed`].
pub fn encode_packed(image: &IndexedImage) -> Result<Vec<u8>, PlanarError> {
    let (width, height) = (image.width, image.height);
    check_geometry(width, height, planar_len(width, height))?;
    if image.pixels.len() != width * height {
        return Err(PlanarError::LengthMismatch {
            len: image.pixels.len(),
            expected: width * height,
        });
    }
    let mut out = vec![0u8; planar_len(width, height)];
    for (i, &px) in image.pixels.iter().enumerate() {
        let (y, x) = (i / width, i % width);
        let group = (y * (width / 8) + x / 8) * 4;
        for plane in 0..4 {
            if px & (1 << plane) != 0 {
                out[group + plane] |= 0x80 >> (x % 8);
            }
        }
    }
    Ok(out)
}

/// How the cells of an archive entry are arranged on the output image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellLayout {
    /// A square sprite of `n × n` cells in row-major cell order (1152 B = 3×3, 2048 B = 4×4,
    /// 4608 B = 6×6; the order was verified visually on the unit sprites).
    Square(usize),
    /// `columns × rows` cells in row-major order (e.g. `HEXZCHR.R3`: 2 × 4 cells = two 32×32
    /// animation frames, one above the other).
    Grid { columns: usize, rows: usize },
    /// Cells in storage order, `columns` per row (chip sheets and undocumented sizes).
    Sheet { columns: usize },
}

impl CellLayout {
    /// Sheet columns for a cell sheet of undocumented shape.
    pub const SHEET_COLUMNS: usize = 16;

    /// Layout for an entry of `cells` cells.
    pub fn for_cells(cells: usize) -> CellLayout {
        match cells {
            9 => CellLayout::Square(3),
            16 => CellLayout::Square(4),
            36 => CellLayout::Square(6),
            n => CellLayout::Sheet {
                columns: n.clamp(1, Self::SHEET_COLUMNS),
            },
        }
    }

    fn columns(self) -> usize {
        match self {
            CellLayout::Square(n) => n,
            CellLayout::Grid { columns, .. } | CellLayout::Sheet { columns } => columns,
        }
    }
}

/// Decode a run of 16×16 cells and lay them out as one image (unused sheet space is index 0).
pub fn cells_to_image(data: &[u8], layout: CellLayout) -> Result<IndexedImage, PlanarError> {
    if data.is_empty() || data.len() % CELL_BYTES != 0 {
        return Err(PlanarError::NotCells { len: data.len() });
    }
    let cells = data.len() / CELL_BYTES;
    let columns = layout.columns().max(1);
    let rows = cells.div_ceil(columns);
    let width = columns * CELL_PX;
    let mut sheet = IndexedImage {
        width,
        height: rows * CELL_PX,
        pixels: vec![0; width * rows * CELL_PX],
    };
    for (i, chunk) in data.chunks_exact(CELL_BYTES).enumerate() {
        let cell = decode(chunk, CELL_PX, CELL_PX)?;
        let (cx, cy) = ((i % columns) * CELL_PX, (i / columns) * CELL_PX);
        for y in 0..CELL_PX {
            let dst = (cy + y) * width + cx;
            sheet.pixels[dst..dst + CELL_PX]
                .copy_from_slice(&cell.pixels[y * CELL_PX..(y + 1) * CELL_PX]);
        }
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: usize, height: usize) -> IndexedImage {
        IndexedImage {
            width,
            height,
            pixels: (0..width * height)
                .map(|i| ((i * 7 + i / width) % 16) as u8)
                .collect(),
        }
    }

    #[test]
    fn documented_bit_layout() {
        // One cell: plane 0 row 0 = 0x80 0x01 -> pixels (0,0) and (15,0) have bit 0;
        // plane 3 row 1 = 0x40 0x00 -> pixel (1,1) has bit 3.
        let mut cell = vec![0u8; CELL_BYTES];
        cell[0] = 0x80;
        cell[1] = 0x01;
        cell[3 * 32 + 2] = 0x40;
        let img = decode(&cell, 16, 16).unwrap();
        assert_eq!(img.pixels[0], 1);
        assert_eq!(img.pixels[15], 1);
        assert_eq!(img.pixels[16 + 1], 8);
        assert_eq!(img.pixels.iter().filter(|&&p| p != 0).count(), 3);
    }

    #[test]
    fn round_trips() {
        for (w, h) in [(16, 16), (64, 80), (8, 1), (48, 48)] {
            let img = gradient(w, h);
            let planar = encode(&img).unwrap();
            assert_eq!(planar.len(), planar_len(w, h));
            assert_eq!(decode(&planar, w, h).unwrap(), img);
        }
        // The portrait geometry of the notes: 64 × 80 × 4 bpp = 2560 bytes.
        assert_eq!(planar_len(64, 80), 2560);
    }

    #[test]
    fn packed_bit_layout_and_round_trip() {
        // 16×1: group 0 = bytes 0..4 (planes 0..3), group 1 = bytes 4..8.
        let data = [0x80, 0x00, 0x00, 0x80, 0x00, 0x01, 0x01, 0x00];
        let img = decode_packed(&data, 16, 1).unwrap();
        assert_eq!(img.pixels[0], 0b1001);
        assert_eq!(img.pixels[15], 0b0110);
        assert_eq!(img.pixels.iter().filter(|&&p| p != 0).count(), 2);
        for (w, h) in [(8, 1), (32, 24), (64, 3)] {
            let img = gradient(w, h);
            let packed = encode_packed(&img).unwrap();
            assert_eq!(decode_packed(&packed, w, h).unwrap(), img);
            // Packed and plane-sequential storage differ once there is more than one group.
            if w * h > 8 {
                assert_ne!(packed, encode(&img).unwrap());
            }
        }
        assert!(decode_packed(&[0; 5], 8, 1).is_err());
        assert!(decode_packed(&[0; 4], 4, 1).is_err());
    }

    #[test]
    fn geometry_errors() {
        assert_eq!(
            decode(&[0; 4], 12, 1).unwrap_err(),
            PlanarError::BadGeometry {
                width: 12,
                height: 1
            }
        );
        assert_eq!(
            decode(&[0; 5], 8, 1).unwrap_err(),
            PlanarError::LengthMismatch {
                len: 5,
                expected: 4
            }
        );
        assert_eq!(
            cells_to_image(&[0; 100], CellLayout::for_cells(1)).unwrap_err(),
            PlanarError::NotCells { len: 100 }
        );
        assert_eq!(
            cells_to_image(&[], CellLayout::for_cells(1)).unwrap_err(),
            PlanarError::NotCells { len: 0 }
        );
    }

    #[test]
    fn layouts() {
        assert_eq!(CellLayout::for_cells(9), CellLayout::Square(3));
        assert_eq!(CellLayout::for_cells(16), CellLayout::Square(4));
        assert_eq!(CellLayout::for_cells(36), CellLayout::Square(6));
        assert_eq!(CellLayout::for_cells(5), CellLayout::Sheet { columns: 5 });
        assert_eq!(
            CellLayout::for_cells(175),
            CellLayout::Sheet { columns: 16 }
        );
    }

    #[test]
    fn cells_are_placed_row_major() {
        // Four cells, each filled with its own index + 1, on a 3-column sheet.
        let mut data = Vec::new();
        for c in 0..4u8 {
            data.extend(
                encode(&IndexedImage {
                    width: 16,
                    height: 16,
                    pixels: vec![c + 1; 256],
                })
                .unwrap(),
            );
        }
        let sheet = cells_to_image(&data, CellLayout::Sheet { columns: 3 }).unwrap();
        assert_eq!((sheet.width, sheet.height), (48, 32));
        let at = |x: usize, y: usize| sheet.pixels[y * 48 + x];
        assert_eq!((at(0, 0), at(20, 5), at(47, 15)), (1, 2, 3));
        assert_eq!((at(5, 20), at(20, 20)), (4, 0));

        let square = cells_to_image(&data, CellLayout::Square(2)).unwrap();
        assert_eq!((square.width, square.height), (32, 32));
        assert_eq!(square.pixels[31 * 32 + 31], 4);

        let tall = cells_to_image(
            &data,
            CellLayout::Grid {
                columns: 1,
                rows: 4,
            },
        )
        .unwrap();
        assert_eq!((tall.width, tall.height), (16, 64));
        assert_eq!(tall.pixels[63 * 16], 4);
    }
}
