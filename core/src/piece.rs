//! Piece preparation: image bytes → background mask → cleaned image, real size and nesting contour.

use image::{GenericImageView, GrayImage, ImageFormat, Luma, RgbaImage};
use imageproc::contours::{BorderType, find_contours};
use imageproc::geometry::{approximate_polygon_dp, contour_area};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// Contour simplification tolerance; small next to the ~1 mm saw separation.
const SIMPLIFY_MM: f64 = 0.05;

/// The real size the jeweller sets: one side in mm, the other follows the proportions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RealSize {
    Width(f64),
    Height(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrepareOptions {
    /// Largest per-channel distance from white still treated as background (0–255).
    /// Ignored when the image already has transparency.
    pub tolerance: u8,
    pub size: RealSize,
    pub allow_rotation: bool,
    pub allow_mirror: bool,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            tolerance: 32,
            size: RealSize::Width(20.0),
            allow_rotation: true,
            allow_mirror: false,
        }
    }
}

/// A cleaned motif, cropped to its mask. Coordinates are in mm, origin top-left, y down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    /// Cleaned PNG: background transparent, interior holes kept visible.
    #[serde(skip)]
    pub png: Vec<u8>,
    pub size_px: [u32; 2],
    pub size_mm: [f64; 2],
    /// Outer silhouette for nesting; interior holes are filled.
    pub contour: Vec<[f64; 2]>,
    pub allow_rotation: bool,
    pub allow_mirror: bool,
}

#[derive(Debug)]
pub enum PrepareError {
    Image(image::ImageError),
    /// Everything was removed as background.
    Empty,
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Image(e) => write!(f, "cannot read the image: {e}"),
            Self::Empty => write!(
                f,
                "no motif found: the whole image was removed as background"
            ),
        }
    }
}

impl std::error::Error for PrepareError {}

impl From<image::ImageError> for PrepareError {
    fn from(e: image::ImageError) -> Self {
        Self::Image(e)
    }
}

pub fn prepare_piece(bytes: &[u8], options: &PrepareOptions) -> Result<Piece, PrepareError> {
    let mut image = image::load_from_memory(bytes)?.to_rgba8();
    let mask = mask(&image, options.tolerance);
    let (x0, y0, w, h) = bounding_box(&mask).ok_or(PrepareError::Empty)?;

    for (pixel, mask_pixel) in image.pixels_mut().zip(mask.pixels()) {
        if mask_pixel[0] == 0 {
            pixel[3] = 0;
        }
    }
    let cleaned: RgbaImage = image.view(x0, y0, w, h).to_image();
    let mut png = Vec::new();
    cleaned.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;

    let (mm_per_px, size_mm) = match options.size {
        RealSize::Width(mm) => (mm / w as f64, [mm, mm * h as f64 / w as f64]),
        RealSize::Height(mm) => (mm / h as f64, [mm * w as f64 / h as f64, mm]),
    };

    Ok(Piece {
        png,
        size_px: [w, h],
        size_mm,
        contour: contour(&mask.view(x0, y0, w, h).to_image(), mm_per_px),
        allow_rotation: options.allow_rotation,
        allow_mirror: options.allow_mirror,
    })
}

/// Foreground = 255. Existing transparency wins; otherwise pixels close to white are background.
fn mask(image: &RgbaImage, tolerance: u8) -> GrayImage {
    // Only when the alpha mask would actually remove something: a stray 254 doesn't count.
    let transparent = image.pixels().any(|p| p[3] < 128);
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        let [r, g, b, a] = image.get_pixel(x, y).0;
        let foreground = if transparent {
            a >= 128
        } else {
            255 - r.min(g).min(b) > tolerance
        };
        Luma([if foreground { 255 } else { 0 }])
    })
}

/// The largest outer border of the mask, simplified, in mm. Following only outer borders fills the holes.
// ponytail: detached parts and specks outside the largest blob are left out of the contour; covering them
// belongs with "split" (#1, stories 8–9).
fn contour(mask: &GrayImage, mm_per_px: f64) -> Vec<[f64; 2]> {
    // A 1 px background frame, so a motif touching the edge still has an outer border.
    let mut framed = GrayImage::new(mask.width() + 2, mask.height() + 2);
    image::imageops::replace(&mut framed, mask, 1, 1);
    let outer = find_contours::<i32>(&framed)
        .into_iter()
        .filter(|c| c.border_type == BorderType::Outer)
        .max_by(|a, b| contour_area(&a.points).total_cmp(&contour_area(&b.points)))
        .expect("a non-empty mask has an outer border");
    approximate_polygon_dp(&outer.points, SIMPLIFY_MM / mm_per_px, true)
        .iter()
        // Border pixel centres, back in unframed coordinates.
        // ponytail: ~0.5 px inside the true edge; negligible at print resolution, which the low-res
        // warning (#1, story 13) enforces.
        .map(|p| {
            [
                (p.x as f64 - 0.5) * mm_per_px,
                (p.y as f64 - 0.5) * mm_per_px,
            ]
        })
        .collect()
}

/// (x, y, width, height) of the foreground, or `None` when there is none.
fn bounding_box(mask: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, _) in mask.enumerate_pixels().filter(|(_, _, p)| p[0] > 0) {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}
