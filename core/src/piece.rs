//! Piece preparation: image bytes → background mask → cleaned image, real size and nesting contours.

use image::{
    DynamicImage, GenericImageView, GrayImage, ImageDecoder, ImageFormat, ImageReader, Luma, Rgba,
    RgbaImage,
};
use imageproc::contours::{BorderType, find_contours};
use imageproc::geometry::{approximate_polygon_dp, contour_area};
use imageproc::region_labelling::{Connectivity, connected_components};
use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

/// Contour simplification tolerance; small next to the ~1 mm saw separation.
const SIMPLIFY_MM: f64 = 0.05;

/// Blobs smaller than this fraction of the largest one are specks (dust, shadows, noise) and are dropped.
// ponytail: relative so it holds before the real size is known; ~1 mm² on a 30 mm motif. Tune here, or
// fix single cases with the brush (#1, story 7).
const SPECK_FRACTION: f64 = 0.002;

/// Below this effective resolution the print gets blurry and the contour inaccurate.
pub const MIN_PPI: f64 = 150.0;

/// Width of a Piece whose file declares no resolution, until the jeweller sets one.
const DEFAULT_WIDTH_MM: f64 = 20.0;

/// Longest side, in px, an SVG's drawing is rasterised to, and the cap for its whole page.
// ponytail: fixed, so a vector stays above MIN_PPI up to ~340 mm unless a tiny drawing sits on a
// huge page; derive them from the real size if that ever matters.
const SVG_PX: f32 = 2048.0;
const SVG_PAGE_PX: f32 = 4096.0;

/// SVG units that give a real size; px (and unitless) are screen units.
const SVG_PHYSICAL_UNITS: [&str; 5] = ["mm", "cm", "in", "pt", "pc"];

/// The real size the jeweller sets: one side in mm, the other follows the proportions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RealSize {
    Width(f64),
    Height(f64),
}

/// Well-known id for the backward-compatible Normal size.
pub const NORMAL_SIZE_ID: &str = "normal";

/// A named real size of a Piece. The first Size is the Default size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    pub id: String,
    pub name: String,
    pub measure: RealSize,
}

/// A Size as it arrives in preparation options: its measure may be missing, in which case the
/// first size is derived from the file's resolution (or the default width) as today.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeInput {
    pub id: String,
    pub name: String,
    pub measure: Option<RealSize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, from = "PrepareOptionsData")]
pub struct PrepareOptions {
    /// Largest per-channel distance from white still treated as background (0–255).
    /// Ignored when the image already has transparency.
    pub tolerance: u8,
    /// The Piece's named Sizes. The first is the Default size, used to set the scale during
    /// preparation. When empty, a single Normal size is created from the legacy `size` field.
    pub sizes: Vec<SizeInput>,
    pub allow_rotation: bool,
    pub allow_mirror: bool,
    /// Manual brush/eraser edits: a PNG the size of the source image. Transparent pixels keep the
    /// automatic mask; painted ones are forced into the piece when green outweighs red (brush), out
    /// of it otherwise (eraser).
    #[serde(skip)]
    pub edits: Option<Vec<u8>>,
}

/// Backward-compatible deserialization: old `source.json` files had a single `size` field.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PrepareOptionsData {
    tolerance: u8,
    size: Option<RealSize>,
    sizes: Vec<SizeInput>,
    allow_rotation: bool,
    allow_mirror: bool,
}

impl Default for PrepareOptionsData {
    fn default() -> Self {
        Self {
            tolerance: 32,
            size: None,
            sizes: Vec::new(),
            allow_rotation: true,
            allow_mirror: false,
        }
    }
}

impl From<PrepareOptionsData> for PrepareOptions {
    fn from(data: PrepareOptionsData) -> Self {
        let sizes = if data.sizes.is_empty() {
            vec![SizeInput {
                id: NORMAL_SIZE_ID.into(),
                name: "Normal".into(),
                measure: data.size,
            }]
        } else {
            data.sizes
        };
        Self {
            tolerance: data.tolerance,
            sizes,
            allow_rotation: data.allow_rotation,
            allow_mirror: data.allow_mirror,
            edits: None,
        }
    }
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            tolerance: 32,
            sizes: Vec::new(),
            allow_rotation: true,
            allow_mirror: false,
            edits: None,
        }
    }
}

/// Backward-compatible deserialization: old `piece.json` files had no `sizes` field.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PieceData {
    #[serde(skip)]
    png: Vec<u8>,
    size_px: [u32; 2],
    #[serde(default)]
    offset_px: [u32; 2],
    #[serde(default)]
    source_px: [u32; 2],
    size_mm: [f64; 2],
    #[serde(default)]
    ppi: f64,
    #[serde(default)]
    low_resolution: bool,
    contours: Vec<Vec<[f64; 2]>>,
    allow_rotation: bool,
    allow_mirror: bool,
    #[serde(default)]
    sizes: Vec<Size>,
}

impl From<PieceData> for Piece {
    fn from(data: PieceData) -> Self {
        let mut sizes = data.sizes;
        if sizes.is_empty() {
            sizes = vec![Size {
                id: NORMAL_SIZE_ID.into(),
                name: "Normal".into(),
                measure: RealSize::Width(data.size_mm[0]),
            }];
        }
        Self {
            png: data.png,
            size_px: data.size_px,
            offset_px: data.offset_px,
            source_px: data.source_px,
            size_mm: data.size_mm,
            ppi: data.ppi,
            low_resolution: data.low_resolution,
            contours: data.contours,
            allow_rotation: data.allow_rotation,
            allow_mirror: data.allow_mirror,
            sizes,
        }
    }
}

/// A cleaned motif, cropped to its mask. Coordinates are in mm, origin top-left, y down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "PieceData")]
pub struct Piece {
    /// Cleaned PNG: background transparent, interior holes kept visible.
    #[serde(skip)]
    pub png: Vec<u8>,
    pub size_px: [u32; 2],
    /// Top-left of the crop in the source image, in px.
    #[serde(default)]
    pub offset_px: [u32; 2],
    /// Size of the decoded source image (upright, SVGs rasterised): what `offset_px` and the
    /// manual edits refer to.
    #[serde(default)]
    pub source_px: [u32; 2],
    pub size_mm: [f64; 2],
    /// Effective resolution at the real size, in px per inch.
    #[serde(default)]
    pub ppi: f64,
    /// `ppi` is below `MIN_PPI`.
    #[serde(default)]
    pub low_resolution: bool,
    /// One outer silhouette per Part, largest first; interior holes are filled. Parts are cut
    /// separately but belong together: nesting places all of them or none.
    pub contours: Vec<Vec<[f64; 2]>>,
    pub allow_rotation: bool,
    pub allow_mirror: bool,
    /// Ordered, non-empty list of named Sizes. The first is the Default size.
    pub sizes: Vec<Size>,
}

impl Piece {
    /// Options that, applied to the Piece's own cleaned image, give it back as it is: how a Piece
    /// whose source was not kept (a split one) reopens for editing.
    pub fn reopen_options(&self) -> PrepareOptions {
        PrepareOptions {
            sizes: self
                .sizes
                .iter()
                .map(|s| SizeInput {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    measure: Some(s.measure),
                })
                .collect(),
            allow_rotation: self.allow_rotation,
            allow_mirror: self.allow_mirror,
            ..Default::default()
        }
    }
}

/// Returns a copy of `piece` scaled to the Size with `size_id`, or `None` if the id is unknown.
/// The contour, dimensions and effective ppi are scaled uniformly; the Size list is reduced to
/// that single Size so callers downstream of nesting/output only ever see one Size.
pub fn piece_at_size(piece: &Piece, size_id: &str) -> Option<Piece> {
    let size = piece.sizes.iter().find(|s| s.id == size_id)?;
    let scale = match size.measure {
        RealSize::Width(mm) => mm / piece.size_mm[0],
        RealSize::Height(mm) => mm / piece.size_mm[1],
    };
    let mut scaled = piece.clone();
    scaled.size_mm = [piece.size_mm[0] * scale, piece.size_mm[1] * scale];
    scaled.ppi = piece.ppi / scale;
    scaled.low_resolution = scaled.ppi < MIN_PPI;
    scaled.contours = piece
        .contours
        .iter()
        .map(|contour| {
            contour
                .iter()
                .map(|&[x, y]| [x * scale, y * scale])
                .collect()
        })
        .collect();
    scaled.sizes = vec![size.clone()];
    Some(scaled)
}

#[derive(Debug)]
pub enum PrepareError {
    Image(image::ImageError),
    Svg(usvg::Error),
    /// Everything was removed as background.
    Empty,
    /// The manual edits are not the size of the source image.
    EditsSize,
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Image(e) => write!(f, "cannot read the image: {e}"),
            Self::Svg(e) => write!(f, "cannot read the SVG: {e}"),
            Self::Empty => write!(
                f,
                "no motif found: the whole image was removed as background"
            ),
            Self::EditsSize => write!(f, "the manual edits do not match the image size"),
        }
    }
}

impl std::error::Error for PrepareError {}

impl From<image::ImageError> for PrepareError {
    fn from(e: image::ImageError) -> Self {
        Self::Image(e)
    }
}

impl From<usvg::Error> for PrepareError {
    fn from(e: usvg::Error) -> Self {
        Self::Svg(e)
    }
}

pub fn prepare_piece(bytes: &[u8], options: &PrepareOptions) -> Result<Piece, PrepareError> {
    let (image, mask, mm_per_px) = clean(bytes, options)?;
    let mut piece = cut_out(&image, &mask, mm_per_px, options)?;
    piece.sizes = options
        .sizes
        .iter()
        .map(|s| Size {
            id: s.id.clone(),
            name: s.name.clone(),
            measure: s.measure.unwrap_or(RealSize::Width(piece.size_mm[0])),
        })
        .collect();
    if piece.sizes.is_empty() {
        piece.sizes = vec![Size {
            id: NORMAL_SIZE_ID.into(),
            name: "Normal".into(),
            measure: RealSize::Width(piece.size_mm[0]),
        }];
    }
    Ok(piece)
}

/// One Piece per Part, at the scale the whole image gets from `options.size`, so the Pieces keep
/// their real sizes relative to each other. Only on request: a motif's detached parts belong together.
pub fn split_piece(bytes: &[u8], options: &PrepareOptions) -> Result<Vec<Piece>, PrepareError> {
    let (image, mask, mm_per_px) = clean(bytes, options)?;
    let labels = connected_components(&mask, Connectivity::Eight, Luma([0u8]));
    let parts = labels.pixels().map(|l| l[0]).max().unwrap_or(0);
    // ponytail: a full-size mask per part, O(parts × pixels); crop each from its component's bounds
    // if images with many parts (lettering) get slow.
    (1..=parts)
        .map(|part| {
            let mask = GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
                Luma([if labels.get_pixel(x, y)[0] == part {
                    255
                } else {
                    0
                }])
            });
            let mut piece = cut_out(&image, &mask, mm_per_px, options)?;
            piece.sizes = vec![Size {
                id: NORMAL_SIZE_ID.into(),
                name: "Normal".into(),
                measure: RealSize::Width(piece.size_mm[0]),
            }];
            Ok(piece)
        })
        .collect()
}

/// The source image, its final mask (edits applied, specks dropped) and the mm per px that gives
/// the whole foreground its real size.
fn clean(
    bytes: &[u8],
    options: &PrepareOptions,
) -> Result<(RgbaImage, GrayImage, f64), PrepareError> {
    let (image, dpi) = decode(bytes)?;
    let mut mask = mask(&image, options.tolerance);
    if let Some(edits) = &options.edits {
        apply_edits(&mut mask, &image::load_from_memory(edits)?.to_rgba8())?;
    }
    // After the edits, so whatever an eraser stroke leaves behind goes too.
    drop_specks(&mut mask);
    let (_, _, w, h) = bounding_box(&mask).ok_or(PrepareError::Empty)?;
    let size = options
        .sizes
        .first()
        .and_then(|s| s.measure)
        .unwrap_or(RealSize::Width(
            dpi.map_or(DEFAULT_WIDTH_MM, |dpi| w as f64 * 25.4 / dpi),
        ));
    let mm_per_px = match size {
        RealSize::Width(mm) => mm / w as f64,
        RealSize::Height(mm) => mm / h as f64,
    };
    Ok((image, mask, mm_per_px))
}

/// The Piece `mask` cuts out of `image`, cropped to it.
fn cut_out(
    image: &RgbaImage,
    mask: &GrayImage,
    mm_per_px: f64,
    options: &PrepareOptions,
) -> Result<Piece, PrepareError> {
    let (x0, y0, w, h) = bounding_box(mask).ok_or(PrepareError::Empty)?;
    let mut cleaned: RgbaImage = image.view(x0, y0, w, h).to_image();
    for (pixel, mask_pixel) in cleaned.pixels_mut().zip(mask.view(x0, y0, w, h).pixels()) {
        if mask_pixel.2[0] == 0 {
            pixel[3] = 0;
        }
    }
    let mut png = Vec::new();
    cleaned.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;

    let ppi = 25.4 / mm_per_px;
    Ok(Piece {
        png,
        size_px: [w, h],
        offset_px: [x0, y0],
        source_px: [image.width(), image.height()],
        size_mm: [w as f64 * mm_per_px, h as f64 * mm_per_px],
        ppi,
        low_resolution: ppi < MIN_PPI,
        contours: contours(&mask.view(x0, y0, w, h).to_image(), mm_per_px),
        allow_rotation: options.allow_rotation,
        allow_mirror: options.allow_mirror,
        sizes: Vec::new(),
    })
}

/// The source image, upright, with the resolution its file declares in px per inch, if any.
/// Anything `image` doesn't recognise is read as an SVG.
fn decode(bytes: &[u8]) -> Result<(RgbaImage, Option<f64>), PrepareError> {
    let Ok(format) = image::guess_format(bytes) else {
        return rasterise_svg(bytes);
    };
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format).into_decoder()?;
    // Phone photos are often stored sideways, with an EXIF tag saying how to turn them.
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok((image.to_rgba8(), dpi(bytes)))
}

/// Rendered on a transparent background, the drawing `SVG_PX` on its longest side. A size declared
/// in physical units (at CSS's 96 px per inch) becomes the resolution.
fn rasterise_svg(bytes: &[u8]) -> Result<(RgbaImage, Option<f64>), PrepareError> {
    // Loading the system fonts takes a while; once per run is enough.
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let options = usvg::Options {
        fontdb: FONTS
            .get_or_init(|| {
                let mut fonts = usvg::fontdb::Database::new();
                fonts.load_system_fonts();
                Arc::new(fonts)
            })
            .clone(),
        ..Default::default()
    };
    let text = std::str::from_utf8(bytes).map_err(|_| usvg::Error::NotAnUtf8Str)?;
    let xml = usvg::roxmltree::Document::parse_with_options(
        text,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(usvg::Error::ParsingFailed)?;
    let physical = xml.root_element().attribute("width").is_some_and(|width| {
        SVG_PHYSICAL_UNITS
            .iter()
            .any(|unit| width.trim().ends_with(unit))
    });
    let tree = usvg::Tree::from_xmltree(&xml, &options)?;
    let (page, drawing) = (tree.size(), tree.root().abs_layer_bounding_box());
    let scale = (SVG_PX / drawing.width().max(drawing.height()))
        .min(SVG_PAGE_PX / page.width().max(page.height()));
    let (w, h) = (
        (page.width() * scale).round() as u32,
        (page.height() * scale).round() as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or(usvg::Error::InvalidSize)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let image = RgbaImage::from_raw(w, h, pixmap.take_demultiplied()).expect("w × h RGBA pixels");
    Ok((image, physical.then_some(96.0 * scale as f64)))
}

/// The resolution in a PNG's pHYs chunk or a JPEG's JFIF header, in px per inch.
// ponytail: JFIF only; a JPEG with just EXIF resolution falls back to DEFAULT_WIDTH_MM.
fn dpi(bytes: &[u8]) -> Option<f64> {
    let be_uint = |at: usize, len: usize| {
        let field = bytes.get(at..at + len)?;
        Some(field.iter().fold(0u32, |n, &b| n << 8 | b as u32) as f64)
    };
    let dpi = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // Chunks: length, type, data, CRC. pHYs must come before the image data.
        let mut at = 8;
        loop {
            match bytes.get(at + 4..at + 8)? {
                // Unit 1 is the metre.
                b"pHYs" if bytes.get(at + 16) == Some(&1) => break be_uint(at + 8, 4)? * 0.0254,
                b"IDAT" => return None,
                _ => at += 12 + be_uint(at, 4)? as usize,
            }
        }
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF, 0xE0]) && bytes.get(6..11) == Some(b"JFIF\0") {
        // Unit 1 is the inch, 2 the centimetre; 0 is only an aspect ratio.
        match bytes.get(13)? {
            1 => be_uint(14, 2)?,
            2 => be_uint(14, 2)? * 2.54,
            _ => return None,
        }
    } else {
        return None;
    };
    (dpi > 0.0).then_some(dpi)
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

fn apply_edits(mask: &mut GrayImage, edits: &RgbaImage) -> Result<(), PrepareError> {
    if edits.dimensions() != mask.dimensions() {
        return Err(PrepareError::EditsSize);
    }
    for (pixel, &Rgba([r, g, _, a])) in mask.pixels_mut().zip(edits.pixels()) {
        if a >= 128 {
            pixel[0] = if g > r { 255 } else { 0 };
        }
    }
    Ok(())
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
