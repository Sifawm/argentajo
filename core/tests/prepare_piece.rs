//! Piece preparation on committed fixtures (see `fixtures/`):
//! - `flower_on_white.png`: 400×300 px, near-white paper (248,248,244); a flower of six r=40 petals at
//!   distance 50 around (200,150), so its bounding box is 180 × 166.6 px.
//! - `transparent.png`: 200×200 px, transparent; a white disc inside a dark ring of outer r=64 at the centre.
//! - `ring.png`: 200×200 px, white; a blue ring, outer r=80 and inner r=35, at the centre.
//! - `flower_with_speck.png`: the flower plus a 3×3 px dark speck at (11..13, 11..13).
//! - `two_parts.png`: 220×120 px, white; separate discs r=40 at (60,60) and r=20 at (170,60).

use argentajo_core::piece::{Piece, PrepareError, PrepareOptions, RealSize, prepare_piece};

fn prepare(fixture: &str, options: PrepareOptions) -> Piece {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    prepare_piece(&std::fs::read(path).unwrap(), &options).unwrap()
}

fn width(mm: f64) -> PrepareOptions {
    PrepareOptions {
        size: RealSize::Width(mm),
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
            size: RealSize::Height(27.77),
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
    let path = format!(
        "{}/tests/fixtures/flower_on_white.png",
        env!("CARGO_MANIFEST_DIR")
    );
    let options = PrepareOptions {
        tolerance: 255,
        ..Default::default()
    };
    assert!(matches!(
        prepare_piece(&std::fs::read(path).unwrap(), &options),
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
