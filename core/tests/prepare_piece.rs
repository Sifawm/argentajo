//! Piece preparation on committed fixtures (see `fixtures/`):
//! - `flower_on_white.png`: 400×300 px, near-white paper (248,248,244); a flower of six r=40 petals at
//!   distance 50 around (200,150), so its bounding box is 180 × 166.6 px.
//! - `transparent.png`: 200×200 px, transparent; a white disc inside a dark ring of outer r=64 at the centre.
//! - `ring.png`: 200×200 px, white; a blue ring, outer r=80 and inner r=35, at the centre.
//! - `flower_with_speck.png`: the flower plus a 3×3 px dark speck at (11..13, 11..13).
//! - `two_parts.png`: 220×120 px, white; separate discs r=40 at (60,60) and r=20 at (170,60).
//! - `flower_on_white.jpg`: the flower as a JPEG, 254 dpi in its JFIF header (10 px/mm).
//! - `ring_254dpi.png`: the ring with a pHYs chunk of 254 dpi (10 px/mm).
//! - `ring.svg`: the ring as a vector on a transparent background, 200×200 user units declared as
//!   20 × 20 mm, so the ring is 16 mm across.
//! - `two_parts_rotated.jpg`: `two_parts.png` stored as is, with EXIF orientation 6 (rotate 90° clockwise).

use argentajo_core::piece::{Piece, PrepareError, PrepareOptions, RealSize, prepare_piece};
use image::{ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn prepare(name: &str, options: PrepareOptions) -> Piece {
    prepare_piece(&fixture(name), &options).unwrap()
}

fn width(mm: f64) -> PrepareOptions {
    PrepareOptions {
        size: Some(RealSize::Width(mm)),
        ..Default::default()
    }
}

fn alpha_at(piece: &Piece, x: u32, y: u32) -> u8 {
    image::load_from_memory(&piece.png)
        .unwrap()
        .to_rgba8()
        .get_pixel(x, y)[3]
}

/// The contour of a single-part piece.
fn only_contour(piece: &Piece) -> &[[f64; 2]] {
    assert_eq!(piece.contours.len(), 1, "expected a single part");
    &piece.contours[0]
}

fn polygon_area(polygon: &[[f64; 2]]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| polygon[i][0] * polygon[(i + 1) % n][1] - polygon[(i + 1) % n][0] * polygon[i][1])
        .sum::<f64>()
        .abs()
        / 2.0
}

fn contains(polygon: &[[f64; 2]], [x, y]: [f64; 2]) -> bool {
    let n = polygon.len();
    (0..n)
        .filter(|&i| {
            let ([xi, yi], [xj, yj]) = (polygon[i], polygon[(i + n - 1) % n]);
            (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi
        })
        .count()
        % 2
        == 1
}

fn assert_near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

#[test]
fn removes_a_white_background_and_crops_to_the_motif() {
    let piece = prepare("flower_on_white.png", width(30.0));

    let [w, h] = piece.size_px;
    assert_near(w as f64, 180.0, 2.0);
    assert_near(h as f64, 166.6, 2.0);
    assert_eq!(
        alpha_at(&piece, 0, 0),
        0,
        "the paper around the petals is removed"
    );
    assert_eq!(alpha_at(&piece, w / 2, h / 2), 255, "the flower is kept");
}

#[test]
fn the_other_side_follows_the_proportions() {
    // 180 × 166.6 px flower.
    let by_width = prepare("flower_on_white.png", width(30.0));
    assert_eq!(by_width.size_mm[0], 30.0);
    assert_near(by_width.size_mm[1], 27.77, 0.4);

    let by_height = prepare(
        "flower_on_white.png",
        PrepareOptions {
            size: Some(RealSize::Height(27.77)),
            ..Default::default()
        },
    );
    assert_eq!(by_height.size_mm[1], 27.77);
    assert_near(by_height.size_mm[0], 30.0, 0.4);
}

#[test]
fn the_contour_encloses_the_motif_in_mm() {
    let piece = prepare("flower_on_white.png", width(30.0));
    let image = image::load_from_memory(&piece.png).unwrap().to_rgba8();
    let mm_per_px = 30.0 / piece.size_px[0] as f64;
    let opaque: Vec<[f64; 2]> = image
        .enumerate_pixels()
        .filter(|(_, _, p)| p[3] > 0)
        .map(|(x, y, _)| [(x as f64 + 0.5) * mm_per_px, (y as f64 + 0.5) * mm_per_px])
        .collect();

    let inside = opaque
        .iter()
        .filter(|&&p| contains(only_contour(&piece), p))
        .count();
    assert!(
        inside as f64 >= 0.98 * opaque.len() as f64,
        "{inside} of {} motif pixels inside",
        opaque.len()
    );
    let motif_area = opaque.len() as f64 * mm_per_px * mm_per_px;
    assert_near(
        polygon_area(only_contour(&piece)),
        motif_area,
        0.03 * motif_area,
    );
}

#[test]
fn interior_holes_are_filled_in_the_contour_but_stay_visible_in_the_image() {
    // Outer r=80 px → 160 px wide; at 16 mm that is 0.1 mm/px.
    let piece = prepare("ring.png", width(16.0));
    let [w, h] = piece.size_px;

    assert_eq!(
        alpha_at(&piece, w / 2, h / 2),
        0,
        "the hole is transparent in the cleaned image"
    );
    assert!(
        contains(only_contour(&piece), [8.0, 8.0]),
        "the hole is inside the contour"
    );
    assert_near(
        polygon_area(only_contour(&piece)),
        std::f64::consts::PI * 8.0 * 8.0,
        3.0,
    );
}

#[test]
fn existing_transparency_is_the_mask_even_for_white_motifs() {
    // Outer r=64 px → 128 px wide; at 12.8 mm that is 0.1 mm/px.
    let piece = prepare(
        "transparent.png",
        PrepareOptions {
            tolerance: 255,
            ..width(12.8)
        },
    );
    let [w, h] = piece.size_px;

    assert_near(w as f64, 128.0, 2.0);
    assert_eq!(
        alpha_at(&piece, w / 2, h / 2),
        255,
        "the white disc is part of the motif"
    );
    assert_near(
        polygon_area(only_contour(&piece)),
        std::f64::consts::PI * 6.4 * 6.4,
        2.0,
    );
}

#[test]
fn tolerance_decides_what_counts_as_background() {
    // The paper is (248,248,244): 11 away from white.
    let strict = prepare(
        "flower_on_white.png",
        PrepareOptions {
            tolerance: 0,
            ..width(30.0)
        },
    );
    assert_eq!(strict.size_px, [400, 300], "nothing is removed");

    let tolerant = prepare(
        "flower_on_white.png",
        PrepareOptions {
            tolerance: 16,
            ..width(30.0)
        },
    );
    assert_near(tolerant.size_px[0] as f64, 180.0, 2.0);
}

#[test]
fn pieces_may_rotate_but_not_mirror_by_default() {
    let piece = prepare("flower_on_white.png", PrepareOptions::default());
    assert!(piece.allow_rotation);
    assert!(!piece.allow_mirror);

    let piece = prepare(
        "flower_on_white.png",
        PrepareOptions {
            allow_rotation: false,
            allow_mirror: true,
            ..Default::default()
        },
    );
    assert!(!piece.allow_rotation);
    assert!(piece.allow_mirror);
}

#[test]
fn an_image_removed_entirely_is_an_error() {
    let options = PrepareOptions {
        tolerance: 255,
        ..Default::default()
    };
    assert!(matches!(
        prepare_piece(&fixture("flower_on_white.png"), &options),
        Err(PrepareError::Empty)
    ));
}

#[test]
fn specks_are_dropped_and_do_not_stretch_the_real_size() {
    let piece = prepare("flower_with_speck.png", width(30.0));

    assert_near(piece.size_px[0] as f64, 180.0, 2.0);
    assert_near(piece.size_px[1] as f64, 166.6, 2.0);
    only_contour(&piece);
}

#[test]
fn separate_parts_each_keep_their_own_contour() {
    // Discs r=40 and r=20 spanning x 20..190 → 170 px wide; at 17 mm that is 0.1 mm/px.
    let piece = prepare("two_parts.png", width(17.0));
    let [big, small] = &piece.contours[..] else {
        panic!("expected two parts, got {}", piece.contours.len())
    };

    assert!(
        contains(big, [4.0, 4.0]),
        "the big disc (centre 40,40 px) is enclosed"
    );
    assert_near(polygon_area(big), std::f64::consts::PI * 4.0 * 4.0, 1.5);
    assert!(
        contains(small, [15.0, 4.0]),
        "the small disc (centre 150,40 px) is enclosed"
    );
    assert_near(polygon_area(small), std::f64::consts::PI * 2.0 * 2.0, 0.6);
    assert!(
        !contains(big, [10.0, 4.0]) && !contains(small, [10.0, 4.0]),
        "the gap between them stays free"
    );
}

const BRUSH: Rgba<u8> = Rgba([0, 255, 0, 255]);
const ERASER: Rgba<u8> = Rgba([255, 0, 0, 255]);

/// Manual edits over a `w`×`h` source as a PNG: each `(tool, [x0, y0, x1, y1])` paints the
/// rectangle x0..x1, y0..y1.
fn edits(w: u32, h: u32, strokes: &[(Rgba<u8>, [u32; 4])]) -> Vec<u8> {
    let mut image = RgbaImage::new(w, h);
    for &(tool, [x0, y0, x1, y1]) in strokes {
        for x in x0..x1 {
            for y in y0..y1 {
                image.put_pixel(x, y, tool);
            }
        }
    }
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();
    png
}

#[test]
fn erasing_removes_an_area_from_the_contour_and_the_image() {
    // Erase the small disc (r=20 at 170,60): only the big one (x 20..100) remains.
    let piece = prepare(
        "two_parts.png",
        PrepareOptions {
            edits: Some(edits(220, 120, &[(ERASER, [145, 35, 195, 85])])),
            ..width(8.0)
        },
    );

    assert_near(piece.size_px[0] as f64, 80.0, 2.0);
    assert_eq!(piece.offset_px, [20, 20], "the crop starts at the big disc");
    assert_near(
        polygon_area(only_contour(&piece)),
        std::f64::consts::PI * 4.0 * 4.0,
        1.5,
    );
}

#[test]
fn brushing_adds_a_missed_area_to_the_contour_and_the_image() {
    // Brush a bridge x 95..155, y 55..65 joining the two discs into one part. The crop starts at
    // (20,20) and is 170 px wide; at 17 mm that is 0.1 mm/px.
    let piece = prepare(
        "two_parts.png",
        PrepareOptions {
            edits: Some(edits(220, 120, &[(BRUSH, [95, 55, 155, 65])])),
            ..width(17.0)
        },
    );

    assert!(
        contains(only_contour(&piece), [10.5, 4.0]),
        "the bridge (centre 125,60 px) is enclosed"
    );
    assert_eq!(alpha_at(&piece, 105, 40), 255, "the bridge is kept");
}

#[test]
fn what_erasing_leaves_behind_is_dropped_as_specks() {
    // Erase the small disc (r=20 at 170,60) around a 3×3 px remnant at (169..172, 59..62).
    let piece = prepare(
        "two_parts.png",
        PrepareOptions {
            edits: Some(edits(
                220,
                120,
                &[
                    (ERASER, [145, 35, 169, 85]),
                    (ERASER, [172, 35, 195, 85]),
                    (ERASER, [169, 35, 172, 59]),
                    (ERASER, [169, 62, 172, 85]),
                ],
            )),
            ..width(8.0)
        },
    );

    only_contour(&piece);
    assert_near(piece.size_px[0] as f64, 80.0, 2.0);
}

#[test]
fn edits_of_another_size_are_an_error() {
    let options = PrepareOptions {
        edits: Some(edits(100, 100, &[])),
        ..Default::default()
    };
    assert!(matches!(
        prepare_piece(&fixture("two_parts.png"), &options),
        Err(PrepareError::EditsSize)
    ));
}

#[test]
fn a_jpg_is_prepared_like_a_png() {
    let piece = prepare("flower_on_white.jpg", width(30.0));

    let [w, h] = piece.size_px;
    assert_near(w as f64, 180.0, 2.0);
    assert_near(h as f64, 166.6, 2.0);
    assert_eq!(alpha_at(&piece, 0, 0), 0, "the paper is removed");
    only_contour(&piece);
}

#[test]
fn a_jpg_is_turned_upright_by_its_exif_orientation() {
    // The two discs side by side (170 × 80 px) end up one above the other.
    let piece = prepare("two_parts_rotated.jpg", width(8.0));

    assert_near(piece.size_px[0] as f64, 80.0, 2.0);
    assert_near(piece.size_px[1] as f64, 170.0, 2.0);
}

#[test]
fn an_svg_on_a_transparent_background_yields_its_silhouette() {
    let piece = prepare("ring.svg", width(16.0));
    let [w, h] = piece.size_px;

    assert_eq!(alpha_at(&piece, w / 2, h / 2), 0, "the hole stays visible");
    assert!(
        contains(only_contour(&piece), [8.0, 8.0]),
        "the hole is filled for nesting"
    );
    assert_near(
        polygon_area(only_contour(&piece)),
        std::f64::consts::PI * 8.0 * 8.0,
        3.0,
    );
}

#[test]
fn the_file_dpi_sets_the_initial_real_size() {
    let svg = prepare("ring.svg", PrepareOptions::default());
    assert_near(svg.size_mm[0], 16.0, 0.2);
    assert_near(svg.size_mm[1], 16.0, 0.2);

    let png = prepare("ring_254dpi.png", PrepareOptions::default());
    assert_near(png.size_mm[0], 16.0, 0.2);

    // 180 px at 10 px/mm.
    let jpg = prepare("flower_on_white.jpg", PrepareOptions::default());
    assert_near(jpg.size_mm[0], 18.0, 0.2);
}

#[test]
fn without_dpi_the_initial_real_size_is_20_mm_wide() {
    let piece = prepare("flower_on_white.png", PrepareOptions::default());
    assert_eq!(piece.size_mm[0], 20.0);
}

#[test]
fn the_real_size_set_by_the_jeweller_overrides_the_file_dpi() {
    let piece = prepare("ring_254dpi.png", width(40.0));
    assert_eq!(piece.size_mm[0], 40.0);
}

#[test]
fn enlarging_past_150_ppi_warns_of_low_resolution() {
    // ~180 px wide, so ~30.5 mm at 150 ppi.
    let px = prepare("flower_on_white.png", width(30.0)).size_px[0] as f64;
    let at_150_ppi = px / 150.0 * 25.4;

    let sharp = prepare("flower_on_white.png", width(at_150_ppi - 0.1));
    assert!(!sharp.low_resolution);
    assert!(sharp.ppi > 150.0);

    let blurry = prepare("flower_on_white.png", width(at_150_ppi + 0.1));
    assert!(blurry.low_resolution);
    assert_near(blurry.ppi, px / ((at_150_ppi + 0.1) / 25.4), 0.01);
}

#[test]
fn an_svg_is_sharp_at_any_jewellery_size() {
    assert!(!prepare("ring.svg", width(150.0)).low_resolution);
}

#[test]
fn a_small_svg_motif_on_a_large_page_stays_sharp() {
    // A 16 mm ring in the corner of a 200 mm page, as drawn on an A4-sized document.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200mm" height="200mm" viewBox="0 0 2000 2000">
        <circle cx="100" cy="100" r="57.5" fill="none" stroke="#1e3a8a" stroke-width="45"/>
    </svg>"##;
    let piece = prepare_piece(svg, &PrepareOptions::default()).unwrap();

    assert_near(piece.size_mm[0], 16.0, 0.2);
    assert!(piece.ppi > 300.0, "only {} ppi", piece.ppi);
}

#[test]
fn an_svg_without_physical_units_has_no_dpi() {
    // A 24-unit icon: no width in mm, cm, in, pt or pc, so no real size to take from it.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
        <circle cx="12" cy="12" r="10" fill="#1e3a8a"/>
    </svg>"##;
    let piece = prepare_piece(svg, &PrepareOptions::default()).unwrap();

    assert_eq!(piece.size_mm[0], 20.0);
}
