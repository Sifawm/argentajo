//! Piece preparation: image bytes → background mask → cleaned image, real size and nesting contours.

use image::{GenericImageView, GrayImage, ImageFormat, Luma, RgbaImage};
use imageproc::contours::{BorderType, find_contours};
use imageproc::geometry::{approximate_polygon_dp, contour_area};
use imageproc::region_labelling::{Connectivity, connected_components};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// Contour simplification tolerance; small next to the ~1 mm saw separation.
const SIMPLIFY_MM: f64 = 0.05;

/// Blobs smaller than this fraction of the largest one are specks (dust, shadows, noise) and are dropped.
// ponytail: relative so it holds before the real size is known; ~1 mm² on a 30 mm motif. Tune here, or
// fix single cases with the brush (#1, story 7).
const SPECK_FRACTION: f64 = 0.002;

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
    /// One outer silhouette per Part, largest first; interior holes are filled. Parts are cut
    /// separately but belong together: nesting places all of them or none.
    pub contours: Vec<Vec<[f64; 2]>>,
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
    let mut mask = mask(&image, options.tolerance);
    drop_specks(&mut mask);
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
        contours: contours(&mask.view(x0, y0, w, h).to_image(), mm_per_px),
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

/// Clears every blob smaller than `SPECK_FRACTION` of the largest one.
fn drop_specks(mask: &mut GrayImage) {
    let labels = connected_components(&*mask, Connectivity::Eight, Luma([0u8]));
    let mut sizes = vec![
        0u32;
        labels
            .pixels()
            .map(|l| l[0] as usize + 1)
            .max()
            .unwrap_or(1)
    ];
    for label in labels.pixels().filter(|l| l[0] > 0) {
        sizes[label[0] as usize] += 1;
    }
    let min = SPECK_FRACTION * sizes[1..].iter().copied().max().unwrap_or(0) as f64;
    for (pixel, label) in mask.pixels_mut().zip(labels.pixels()) {
        if (sizes[label[0] as usize] as f64) < min {
            pixel[0] = 0;
        }
    }
}

/// Each blob's outer border, simplified, in mm, largest first. Following only outer borders fills the holes.
fn contours(mask: &GrayImage, mm_per_px: f64) -> Vec<Vec<[f64; 2]>> {
    // A 1 px background frame, so a motif touching the edge still has an outer border.
    let mut framed = GrayImage::new(mask.width() + 2, mask.height() + 2);
    image::imageops::replace(&mut framed, mask, 1, 1);
    let mut outers: Vec<_> = find_contours::<i32>(&framed)
        .into_iter()
        .filter(|c| c.border_type == BorderType::Outer)
        .map(|c| c.points)
        .collect();
    outers.sort_by(|a, b| contour_area(b).total_cmp(&contour_area(a)));
    outers
        .iter()
        .map(|outer| {
            approximate_polygon_dp(outer, SIMPLIFY_MM / mm_per_px, true)
                .iter()
                // Border pixel centres, back in unframed coordinates.
                // ponytail: ~0.5 px inside the true edge; negligible at print resolution, which the
                // low-res warning (#1, story 13) enforces.
                .map(|p| {
                    [
                        (p.x as f64 - 0.5) * mm_per_px,
                        (p.y as f64 - 0.5) * mm_per_px,
                    ]
                })
                .collect()
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
