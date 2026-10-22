//! Nesting properties, on synthetic pieces with exact contours. Nesting is stochastic, so these
//! assert invariants (no overlap, containment, separation, flags, all-or-nothing Parts, the missing
//! report, utilisation, Sheets on the page) over fixed seeds rather than coordinates.

use argentajo_core::nest::Quantity::{Fill, Fixed};
use argentajo_core::nest::{
    Job, JobItem, Layout, NestError, Paper, Placement, Quantity, SheetLayout, nest,
};
use argentajo_core::output::{OutputOptions, render_page};
use argentajo_core::piece::{Piece, PrepareOptions, RealSize, prepare_piece};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const SEEDS: [u64; 2] = [1, 2];
const TIME_LIMIT: Duration = Duration::from_secs(2);
/// jagua-rs rounds its separation offsets with 0.1 rad chords and works in f32.
const TOLERANCE: f64 = 0.01;

/// Between Sheets on the page.
const SHEET_GAP: f64 = 5.0;

const A4: Paper = Paper {
    size_mm: [210.0, 297.0],
    margin_mm: 5.0,
};

fn rect(w: f64, h: f64) -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
}

/// An asymmetric L: any rotation or mirroring shows.
fn ell() -> Vec<[f64; 2]> {
    vec![
        [0.0, 0.0],
        [6.0, 0.0],
        [6.0, 14.0],
        [16.0, 14.0],
        [16.0, 20.0],
        [0.0, 20.0],
    ]
}

fn triangle() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [18.0, 0.0], [0.0, 9.0]]
}

fn piece(contours: Vec<Vec<[f64; 2]>>, allow_rotation: bool, allow_mirror: bool) -> Piece {
    let (w, h) = contours
        .iter()
        .flatten()
        .fold((0.0f64, 0.0f64), |(w, h), &[x, y]| (w.max(x), h.max(y)));
    Piece {
        png: vec![],
        size_px: [w as u32 * 10, h as u32 * 10],
        offset_px: [0, 0],
        source_px: [w as u32 * 10, h as u32 * 10],
        size_mm: [w, h],
        ppi: 254.0,
        low_resolution: false,
        contours,
        allow_rotation,
        allow_mirror,
    }
}

fn job(sheet_mm: [f64; 2], items: Vec<(Piece, Quantity)>) -> Job {
    Job {
        paper: A4,
        sheets_mm: vec![sheet_mm],
        separation_mm: 1.0,
        edge_margin_mm: 1.5,
        time_limit: TIME_LIMIT,
        items: items
            .into_iter()
            .map(|(piece, quantity)| JobItem { piece, quantity })
            .collect(),
    }
}

/// The placed Part's contour, in Sheet mm.
fn placed(job: &Job, p: &Placement) -> Vec<[f64; 2]> {
    let [a, b, c, d, e, f] = p.transform;
    job.items[p.item].piece.contours[p.part]
        .iter()
        .map(|&[x, y]| [a * x + c * y + e, b * x + d * y + f])
        .collect()
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

fn edges(polygon: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    (0..polygon.len()).map(|i| (polygon[i], polygon[(i + 1) % polygon.len()]))
}

/// Positive when `r` is left of `p → q`.
fn cross(p: [f64; 2], q: [f64; 2], r: [f64; 2]) -> f64 {
    (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
}

fn point_segment_distance(p: [f64; 2], (a, b): ([f64; 2], [f64; 2])) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

fn segment_distance(s: ([f64; 2], [f64; 2]), t: ([f64; 2], [f64; 2])) -> f64 {
    let crosses = cross(s.0, s.1, t.0) * cross(s.0, s.1, t.1) < 0.0
        && cross(t.0, t.1, s.0) * cross(t.0, t.1, s.1) < 0.0;
    if crosses {
        return 0.0;
    }
    [
        point_segment_distance(s.0, t),
        point_segment_distance(s.1, t),
        point_segment_distance(t.0, s),
        point_segment_distance(t.1, s),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

/// 0 when they overlap.
fn distance(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    if a.iter().any(|&p| contains(b, p)) || b.iter().any(|&p| contains(a, p)) {
        return 0.0;
    }
    edges(a)
        .flat_map(|s| edges(b).map(move |t| segment_distance(s, t)))
        .fold(f64::INFINITY, f64::min)
}

/// Every invariant a Layout must hold, whatever the seed.
fn assert_valid(job: &Job, layout: &Layout) {
    assert_sheets_on_pages(job, layout);
    let mut area = 0.0;
    for (s, (sheet, &[w, h])) in layout.sheets.iter().zip(&job.sheets_mm).enumerate() {
        let m = job.edge_margin_mm - TOLERANCE;
        let polygons: Vec<_> = sheet.placements.iter().map(|p| placed(job, p)).collect();

        for (p, polygon) in sheet.placements.iter().zip(&polygons) {
            assert!(
                polygon
                    .iter()
                    .all(|&[x, y]| x >= m && x <= w - m && y >= m && y <= h - m),
                "sheet {s}: {p:?} is not inside the sheet minus the edge margin"
            );
            let [a, b, c, d, ..] = p.transform;
            let piece = &job.items[p.item].piece;
            if !piece.allow_rotation {
                // Only a left-right flip, if any, keeps it upright.
                assert!(
                    b.abs() < 1e-6 && c.abs() < 1e-6 && (d - 1.0).abs() < 1e-6,
                    "{p:?} rotated"
                );
                assert!((a.abs() - 1.0).abs() < 1e-6, "{p:?} scaled");
            }
            if !piece.allow_mirror {
                assert!(a * d - b * c > 0.0, "{p:?} mirrored");
            }
        }

        for i in 0..polygons.len() {
            for j in i + 1..polygons.len() {
                let gap = distance(&polygons[i], &polygons[j]);
                assert!(
                    gap >= job.separation_mm - TOLERANCE,
                    "sheet {s}: {:?} and {:?} are {gap} mm apart",
                    sheet.placements[i],
                    sheet.placements[j]
                );
            }
        }

        for (i, item) in job.items.iter().enumerate() {
            let copies = copies_on(sheet, i);
            for part in 0..item.piece.contours.len() {
                let n = sheet
                    .placements
                    .iter()
                    .filter(|p| p.item == i && p.part == part)
                    .count();
                assert_eq!(
                    n, copies,
                    "sheet {s}, item {i}, part {part}: a copy's Parts share its Sheet"
                );
            }
        }
        area += polygons.iter().map(|p| polygon_area(p)).sum::<f64>();
    }

    for (i, item) in job.items.iter().enumerate() {
        match item.quantity {
            Fixed(quantity) => assert_eq!(
                layout.placed[i] + layout.missing[i],
                quantity,
                "item {i}: placed and missing make up the quantity"
            ),
            Fill => assert_eq!(layout.missing[i], 0, "item {i}: fill is never missing"),
        }
        let copies: usize = layout.sheets.iter().map(|sheet| copies_on(sheet, i)).sum();
        assert_eq!(
            copies, layout.placed[i] as usize,
            "item {i}: one per placed copy"
        );
    }

    let silver: f64 = job.sheets_mm.iter().map(|[w, h]| w * h).sum();
    assert!(
        (layout.utilisation - area / silver).abs() < 1e-9,
        "utilisation {} is not placed area / sheets' area {}",
        layout.utilisation,
        area / silver
    );
}

/// Copies of item `i` on the Sheet, counted by its first Part.
fn copies_on(sheet: &SheetLayout, i: usize) -> usize {
    sheet
        .placements
        .iter()
        .filter(|p| p.item == i && p.part == 0)
        .count()
}

/// Each Sheet lies on its page inside the printer margin, apart from the others on that page.
fn assert_sheets_on_pages(job: &Job, layout: &Layout) {
    let [page_w, page_h] = job.paper.size_mm;
    let m = job.paper.margin_mm;
    assert_eq!(layout.sheets.len(), job.sheets_mm.len(), "one per Sheet");
    let rects: Vec<_> = layout
        .sheets
        .iter()
        .zip(&job.sheets_mm)
        .map(|(sheet, &[w, h])| {
            let [x, y] = sheet.origin_mm;
            (sheet.page, [x, y, x + w, y + h])
        })
        .collect();
    for (s, &(page, [x0, y0, x1, y1])) in rects.iter().enumerate() {
        assert!(page < layout.pages(), "sheet {s} on page {page}");
        assert!(
            x0 >= m - 1e-9 && y0 >= m - 1e-9 && x1 <= page_w - m + 1e-9 && y1 <= page_h - m + 1e-9,
            "sheet {s} at {:?} is not inside the printer margin",
            [x0, y0, x1, y1]
        );
        for &(other_page, [u0, v0, u1, v1]) in &rects[s + 1..] {
            let apart = (u0 - x1).max(x0 - u1).max(v0 - y1).max(y0 - v1);
            assert!(
                page != other_page || apart >= SHEET_GAP - 1e-9,
                "sheet {s} is {apart} mm from a later one on page {page}"
            );
        }
    }
}

fn nest_valid(job: &Job, seed: u64) -> Layout {
    let layout = try_nest(job, seed).unwrap();
    assert_valid(job, &layout);
    layout
}

fn try_nest(job: &Job, seed: u64) -> Result<Layout, NestError> {
    nest(
        job,
        seed,
        |_| {},
        &AtomicBool::new(false),
        &AtomicU64::new(0),
    )
}

fn mixed_items() -> Vec<(Piece, Quantity)> {
    vec![
        (piece(vec![ell()], true, false), Fixed(5)),
        (piece(vec![triangle()], false, false), Fixed(4)),
        (piece(vec![ell()], false, true), Fixed(3)),
        (
            piece(vec![rect(12.0, 12.0), rect(5.0, 5.0)], true, false),
            Fixed(3),
        ),
    ]
}

/// More than fits, so the run has plenty to improve on.
fn crowded_job(time_limit: Duration) -> Job {
    let mut job = job(
        [120.0, 70.0],
        mixed_items()
            .into_iter()
            .map(|(piece, quantity)| match quantity {
                Fixed(n) => (piece, Fixed(n * 6)),
                Fill => (piece, Fill),
            })
            .collect(),
    );
    job.time_limit = time_limit;
    job
}

#[test]
fn places_mixed_pieces_on_a_landscape_sheet() {
    for seed in SEEDS {
        let job = job([120.0, 70.0], mixed_items());
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.missing, [0, 0, 0, 0], "seed {seed}: there is room");
        assert_eq!(layout.placed, [5, 4, 3, 3]);
    }
}

#[test]
fn keeps_upright_pieces_upright_on_a_portrait_sheet() {
    for seed in SEEDS {
        let job = job([70.0, 120.0], mixed_items());
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.missing, [0, 0, 0, 0], "seed {seed}: there is room");
    }
}

#[test]
fn compacts_towards_one_end_of_the_sheet() {
    for (sheet, long_axis) in [([200.0, 50.0], 0), ([50.0, 200.0], 1)] {
        let job = job(
            sheet,
            vec![(piece(vec![rect(10.0, 10.0)], false, false), Fixed(3))],
        );
        let layout = nest_valid(&job, 1);
        let far = layout.sheets[0]
            .placements
            .iter()
            .flat_map(|p| placed(&job, p))
            .map(|point| point[long_axis])
            .fold(0.0, f64::max);
        assert!(far < 50.0, "{sheet:?}: reaches {far} mm along the sheet");
    }
}

#[test]
fn copies_that_do_not_fit_are_missing() {
    // 47 × 27 mm inside the edge margin: two rows of four 10 mm squares at 1 mm apart.
    for seed in SEEDS {
        let job = job(
            [50.0, 30.0],
            vec![(piece(vec![rect(10.0, 10.0)], false, false), Fixed(12))],
        );
        let layout = nest_valid(&job, seed);
        assert!(layout.missing[0] >= 4, "seed {seed}: {:?}", layout.missing);
        assert!(layout.placed[0] >= 6, "seed {seed}: {:?}", layout.placed);
    }
}

#[test]
fn a_piece_larger_than_the_sheet_is_missing() {
    for seed in SEEDS {
        let job = job(
            [50.0, 30.0],
            vec![
                (piece(vec![rect(60.0, 40.0)], true, false), Fixed(2)),
                (piece(vec![rect(10.0, 10.0)], true, false), Fixed(2)),
            ],
        );
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.missing, [2, 0], "seed {seed}");
    }
}

#[test]
fn a_copy_is_placed_with_all_its_parts_or_missing() {
    // Room for at most one 20 mm part; the 4 mm ones would fit many times over.
    for seed in SEEDS {
        let job = job(
            [30.0, 30.0],
            vec![(
                piece(vec![rect(20.0, 20.0), rect(4.0, 4.0)], false, false),
                Fixed(3),
            )],
        );
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.placed, [1], "seed {seed}");
        assert_eq!(layout.missing, [2], "seed {seed}");
    }
}

#[test]
fn rejects_an_empty_sheet_or_negative_spacing() {
    assert!(try_nest(&job([0.0, 50.0], vec![]), 1).is_err());
    let mut negative = job([100.0, 50.0], vec![]);
    negative.edge_margin_mm = -1.0;
    assert!(try_nest(&negative, 1).is_err());
}

#[test]
fn a_piece_that_only_just_fits_across_is_placed_or_missing() {
    // 27 mm across inside the edge margin: 26.98 mm passes the pre-check with nothing to spare.
    for seed in SEEDS {
        let job = job(
            [50.0, 30.0],
            vec![(piece(vec![rect(26.98, 26.98)], false, false), Fixed(1))],
        );
        nest_valid(&job, seed);
    }
}

#[test]
fn rejects_a_sheet_larger_than_the_paper() {
    let job = job([250.0, 100.0], vec![]);
    assert!(try_nest(&job, 1).is_err());
}

#[test]
fn renders_the_page_in_mm_with_every_placed_part() {
    let ring = prepare_piece(
        &std::fs::read(format!(
            "{}/tests/fixtures/two_parts.png",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
        &PrepareOptions {
            size: Some(RealSize::Width(30.0)),
            ..Default::default()
        },
    )
    .unwrap();
    let job = job([100.0, 60.0], vec![(ring, Fixed(3))]);
    let layout = nest_valid(&job, 1);
    assert_eq!(layout.sheets[0].placements.len(), 6);

    let svg = render_page(&job, &layout, 0, &OutputOptions::default());
    let tree = resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();
    let px_per_mm = 96.0 / 25.4;
    assert!((tree.size().width() - 210.0 * px_per_mm).abs() < 0.01);
    assert!((tree.size().height() - 297.0 * px_per_mm).abs() < 0.01);
    assert_eq!(svg.matches("<use ").count(), 6, "one image per placed part");
}

#[test]
fn reports_every_improved_layout_and_returns_the_best() {
    let mut items = mixed_items();
    items.push((piece(vec![triangle()], true, true), Fill));
    let job = job([120.0, 70.0], items);
    let mut reported = vec![];
    let iterations = AtomicU64::new(0);
    let layout = nest(
        &job,
        1,
        |l| reported.push(l.clone()),
        &AtomicBool::new(false),
        &iterations,
    )
    .unwrap();
    assert!(
        !reported.is_empty(),
        "at least the first layout is reported"
    );
    assert!(
        iterations.into_inner() > reported.len() as u64,
        "sparrow's iterations are counted"
    );
    for l in &reported {
        assert_valid(&job, l);
    }
    for pair in reported.windows(2) {
        assert!(
            pair[1].utilisation >= pair[0].utilisation - 1e-9,
            "utilisation went down: {} then {}",
            pair[0].utilisation,
            pair[1].utilisation
        );
    }
    assert_eq!(
        reported.last(),
        Some(&layout),
        "the best is the last reported"
    );
}

#[test]
fn cancelling_mid_run_returns_a_valid_layout() {
    let job = crowded_job(Duration::from_secs(60));
    let cancel = AtomicBool::new(false);
    let mut cancelled = None;
    let mut reported = vec![];
    let layout = nest(
        &job,
        1,
        |l| {
            reported.push(l.clone());
            cancel.store(true, Ordering::Relaxed);
            _ = cancelled.get_or_insert_with(Instant::now);
        },
        &cancel,
        &AtomicU64::new(0),
    )
    .unwrap();
    // The first Layout is always waited for: timed from the cancel, not the start.
    let after = cancelled.unwrap().elapsed();
    assert!(
        after < Duration::from_secs(1),
        "took {after:?} after the cancel"
    );
    assert_valid(&job, &layout);
    assert_eq!(reported.last(), Some(&layout), "the best so far is kept");
    assert!(layout.placed.iter().sum::<u32>() > 0);
}

#[test]
fn never_runs_past_the_time_limit() {
    let job = crowded_job(Duration::from_secs(2));
    let start = Instant::now();
    let mut first = None;
    let layout = nest(
        &job,
        1,
        |_| _ = first.get_or_insert_with(|| start.elapsed()),
        &AtomicBool::new(false),
        &AtomicU64::new(0),
    )
    .unwrap();
    // The first Layout is always waited for; there is nothing valid to return before it.
    let bound = job.time_limit.max(first.unwrap()) + Duration::from_millis(200);
    assert!(
        start.elapsed() < bound,
        "took {:?}, first layout at {first:?}",
        start.elapsed()
    );
    assert_valid(&job, &layout);
}

#[test]
fn fill_adds_copies_when_there_is_room() {
    for seed in SEEDS {
        let job = job(
            [100.0, 50.0],
            vec![(piece(vec![rect(10.0, 10.0)], false, false), Fill)],
        );
        let layout = nest_valid(&job, seed);
        // 97 × 47 mm inside the edge margin: four rows of eight 10 mm squares at 1 mm apart.
        assert!(layout.placed[0] >= 16, "seed {seed}: {:?}", layout.placed);
    }
}

#[test]
fn fixed_items_are_placed_before_fill() {
    // Room for eight 10 mm squares: the six fixed ones come first, fill takes what's left.
    for seed in SEEDS {
        let job = job(
            [50.0, 30.0],
            vec![
                (piece(vec![rect(10.0, 10.0)], false, false), Fill),
                (piece(vec![rect(10.0, 10.0)], false, false), Fixed(6)),
            ],
        );
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.placed[1], 6, "seed {seed}");
        assert!(layout.placed[0] >= 1, "seed {seed}: {:?}", layout.placed);
    }
}

#[test]
fn fill_respects_flags_and_parts_around_fixed_items() {
    for (seed, sheet) in SEEDS.into_iter().zip([[120.0, 70.0], [70.0, 120.0]]) {
        let mut items = mixed_items();
        items.extend([
            (piece(vec![ell()], false, false), Fill),
            (piece(vec![triangle()], true, true), Fill),
            (
                piece(vec![rect(8.0, 8.0), rect(3.0, 3.0)], false, false),
                Fill,
            ),
        ]);
        let job = job(sheet, items);
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.missing, [0; 7], "{sheet:?}");
        assert!(
            layout.placed[4..].iter().all(|&n| n >= 1),
            "{sheet:?}: {:?}",
            layout.placed
        );
    }
}

#[test]
fn fill_alone_keeps_improving_until_the_time_limit_and_repeats_with_the_seed() {
    let mut job = job(
        [100.0, 50.0],
        vec![(piece(vec![triangle()], true, true), Fill)],
    );
    job.time_limit = Duration::from_secs(1);
    let run = || {
        let start = Instant::now();
        let mut reported = vec![];
        let iterations = AtomicU64::new(0);
        let layout = nest(
            &job,
            1,
            |l| reported.push(l.clone()),
            &AtomicBool::new(false),
            &iterations,
        )
        .unwrap();
        assert!(iterations.into_inner() > 1, "one per fill");
        assert_valid(&job, &layout);
        assert!(
            start.elapsed() >= job.time_limit,
            "stopped after {:?}",
            start.elapsed()
        );
        assert_eq!(reported.last(), Some(&layout));
        for pair in reported.windows(2) {
            assert!(pair[1].utilisation > pair[0].utilisation);
        }
        reported
    };
    assert_eq!(run()[0], run()[0], "same seed, same first layout");
}

#[test]
fn fill_that_does_not_fit_is_not_missing() {
    let job = job(
        [50.0, 30.0],
        vec![(piece(vec![rect(60.0, 40.0)], true, false), Fill)],
    );
    let layout = nest_valid(&job, 1);
    assert_eq!((layout.placed[0], layout.missing[0]), (0, 0));
}

fn square(size: f64, quantity: Quantity) -> (Piece, Quantity) {
    (piece(vec![rect(size, size)], false, false), quantity)
}

#[test]
fn sheets_go_in_rows_inside_the_margin_and_overflow_onto_new_pages() {
    // 200 × 287 mm inside the printer margin: two 90 mm Sheets across, three rows down.
    let mut job = job([90.0, 90.0], vec![]);
    job.sheets_mm = vec![[90.0, 90.0]; 7];
    let layout = nest_valid(&job, 1);
    assert_eq!(layout.pages(), 2);
    let pages: Vec<_> = layout.sheets.iter().map(|sheet| sheet.page).collect();
    assert_eq!(pages, [0, 0, 0, 0, 0, 0, 1]);
    assert_eq!(layout.sheets[0].origin_mm, [5.0, 5.0], "from the top left");
    assert_eq!(
        layout.sheets[6].origin_mm,
        [5.0, 5.0],
        "a new page starts again"
    );
    assert_eq!(layout.sheets[1].origin_mm[1], 5.0, "same row");

    // Each page shows only its own Sheets' outlines.
    let outlines = |page| {
        render_page(&job, &layout, page, &OutputOptions::default())
            .matches(r#"stroke="black""#)
            .count()
    };
    assert_eq!((outlines(0), outlines(1)), (6, 1));
}

#[test]
fn sheets_leave_room_for_the_verification_square() {
    // 200 × 287 mm inside the printer margin; the square takes its bottom-right 10 mm, 5 mm apart.
    let over_it = job([200.0, 287.0], vec![]);
    assert!(matches!(
        try_nest(&over_it, 1),
        Err(NestError::SheetOffPaper)
    ));
    nest_valid(&job([200.0, 272.0], vec![]), 1);
    nest_valid(&job([185.0, 287.0], vec![]), 1);
}

#[test]
fn a_sheet_wider_than_a_row_starts_the_next_one() {
    let mut job = job([90.0, 90.0], vec![]);
    job.sheets_mm = vec![[90.0, 40.0], [150.0, 60.0], [40.0, 30.0]];
    let layout = nest_valid(&job, 1);
    assert_eq!(layout.pages(), 1);
    assert!(layout.sheets[1].origin_mm[1] >= 45.0, "below the first");
    assert!(layout.sheets[2].origin_mm[0] > 150.0, "beside the second");
}

#[test]
fn rejects_a_job_without_sheets() {
    let mut job = job([90.0, 90.0], vec![]);
    job.sheets_mm.clear();
    assert!(matches!(try_nest(&job, 1), Err(NestError::InvalidSettings)));
}

#[test]
fn fixed_items_stay_on_the_first_sheet_while_they_fit() {
    for seed in SEEDS {
        let mut job = job([50.0, 30.0], vec![square(10.0, Fixed(3))]);
        job.sheets_mm.push([50.0, 30.0]);
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.placed, [3], "seed {seed}");
        assert!(layout.sheets[1].placements.is_empty(), "seed {seed}");
    }
}

#[test]
fn fixed_items_fill_the_sheets_in_order() {
    // 27 mm inside the edge margin: room for one 20 mm square per Sheet.
    for seed in SEEDS {
        let mut job = job([30.0, 30.0], vec![square(20.0, Fixed(3))]);
        job.sheets_mm = vec![[30.0, 30.0]; 4];
        let layout = nest_valid(&job, seed);
        let on: Vec<_> = layout.sheets.iter().map(|s| s.placements.len()).collect();
        assert_eq!(on, [1, 1, 1, 0], "seed {seed}");
    }
}

#[test]
fn what_fits_no_sheet_is_missing() {
    for seed in SEEDS {
        let mut job = job([50.0, 30.0], vec![square(10.0, Fixed(20))]);
        job.sheets_mm.push([50.0, 30.0]);
        let layout = nest_valid(&job, seed);
        assert!(layout.missing[0] >= 4, "seed {seed}: {:?}", layout.missing);
    }
}

#[test]
fn a_piece_too_large_for_one_sheet_goes_on_another() {
    for seed in SEEDS {
        let mut job = job(
            [30.0, 30.0],
            vec![square(25.0, Fixed(1)), square(40.0, Fixed(1))],
        );
        job.sheets_mm.push([100.0, 50.0]);
        let layout = nest_valid(&job, seed);
        assert_eq!(layout.missing, [0, 0], "seed {seed}");
        assert!(
            layout.sheets[0].placements.iter().all(|p| p.item == 0),
            "seed {seed}: only the small one fits the first sheet"
        );
    }
}

#[test]
fn fill_uses_every_sheet() {
    let mut job = job(
        [50.0, 30.0],
        vec![square(10.0, Fixed(8)), square(10.0, Fill)],
    );
    job.sheets_mm.push([50.0, 30.0]);
    let layout = nest_valid(&job, 1);
    assert!(
        layout.sheets[1].placements.iter().any(|p| p.item == 1),
        "{:?}",
        layout.placed
    );
}

#[test]
fn sheets_left_empty_do_not_cut_the_first_one_short() {
    // Twenty Ls take about two fifths of the first Sheet; sparrow keeps improving them for seconds.
    let ells = (piece(vec![ell()], true, false), Fixed(20));
    let mut job = job([120.0, 70.0], vec![ells]);
    job.sheets_mm = vec![[120.0, 70.0]; 3];
    job.time_limit = Duration::from_secs(3);
    let start = Instant::now();
    let layout = nest_valid(&job, 1);
    assert_eq!(layout.missing, [0]);
    assert!(layout.sheets[1].placements.is_empty());
    assert!(
        start.elapsed() > job.time_limit * 2 / 3,
        "stopped after {:?}: the first Sheet only got its share of three",
        start.elapsed()
    );
}
