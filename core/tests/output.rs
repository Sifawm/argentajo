//! Printed output: each page as an SVG in mm and the PDF made from them, on hand-built Layouts.

use argentajo_core::nest::Quantity::Fixed;
use argentajo_core::nest::{Job, JobItem, Layout, Paper, Placement, SheetLayout};
use argentajo_core::output::{OutputOptions, export_pdf, render_page};
use argentajo_core::piece::{NORMAL_SIZE_ID, PrepareOptions, RealSize, SizeInput, prepare_piece};
use resvg::usvg::Tree;
use std::time::Duration;

const PX_PER_MM: f32 = 96.0 / 25.4;
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A5 landscape, two 90 × 60 mm Sheets on the first page and one on the second, a ring on each.
fn job_and_layout() -> (Job, Layout) {
    let ring = prepare_piece(
        &std::fs::read(format!(
            "{}/tests/fixtures/ring.png",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
        &PrepareOptions {
            sizes: vec![SizeInput {
                id: NORMAL_SIZE_ID.into(),
                name: "Normal".into(),
                measure: Some(RealSize::Width(20.0)),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let job = Job {
        paper: Paper {
            size_mm: [210.0, 148.0],
            margin_mm: 5.0,
        },
        sheets_mm: vec![[90.0, 60.0]; 3],
        separation_mm: 1.0,
        edge_margin_mm: 1.5,
        time_limit: Duration::from_secs(1),
        rotation_step_deg: 1.0,
        items: vec![JobItem {
            piece: ring,
            size_id: NORMAL_SIZE_ID.into(),
            quantity: Fixed(3),
        }],
    };
    let sheet = |page, origin_mm, transform| SheetLayout {
        page,
        origin_mm,
        placements: vec![Placement {
            item: 0,
            part: 0,
            transform,
        }],
    };
    let layout = Layout {
        sheets: vec![
            sheet(0, [5.0, 5.0], [1.0, 0.0, 0.0, 1.0, 2.0, 2.0]),
            // Rotated a quarter turn and mirrored.
            sheet(0, [100.0, 5.0], [0.0, 1.0, 1.0, 0.0, 2.0, 2.0]),
            sheet(1, [5.0, 5.0], [-1.0, 0.0, 0.0, 1.0, 30.0, 2.0]),
        ],
        placed: vec![3],
        missing: vec![0],
        utilisation: 0.1,
    };
    (job, layout)
}

fn tree(svg: &str) -> Tree {
    Tree::from_str(svg, &Default::default()).unwrap()
}

#[test]
fn every_page_is_the_paper_size() {
    let (job, layout) = job_and_layout();
    for page in 0..layout.pages() {
        let tree = tree(&render_page(&job, &layout, page, &OutputOptions::default()));
        assert!((tree.size().width() / PX_PER_MM - 210.0).abs() < 1e-3);
        assert!((tree.size().height() / PX_PER_MM - 148.0).abs() < 1e-3);
    }
}

#[test]
fn options_toggle_the_sheet_outlines_and_the_cutting_silhouettes() {
    let (job, layout) = job_and_layout();
    let count = |options: OutputOptions, class: &str| {
        render_page(&job, &layout, 0, &options)
            .matches(&format!(r#"class="{class}""#))
            .count()
    };
    let default = OutputOptions::default();
    assert_eq!(
        (count(default, "sheet"), count(default, "silhouette")),
        (2, 0),
        "outlines on and silhouettes off by default"
    );
    let flipped = OutputOptions {
        sheet_outline: false,
        silhouette: true,
    };
    assert_eq!(
        (count(flipped, "sheet"), count(flipped, "silhouette")),
        (0, 2)
    );
}

/// The pages' MediaBoxes, in pt.
fn media_boxes(pdf: &[u8]) -> Vec<[f64; 4]> {
    let pdf = String::from_utf8_lossy(pdf);
    pdf.split("/MediaBox [")
        .skip(1)
        .map(|rest| {
            let numbers: Vec<f64> = rest[..rest.find(']').unwrap()]
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            numbers.try_into().unwrap()
        })
        .collect()
}

#[test]
fn exports_one_pdf_page_per_paper_page_at_the_paper_size() {
    let (job, layout) = job_and_layout();
    let pdf = export_pdf(&job, &layout, &OutputOptions::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let boxes = media_boxes(&pdf);
    assert_eq!(boxes.len(), 2, "one page per paper page");
    for [x, y, w, h] in boxes {
        assert_eq!((x, y), (0.0, 0.0));
        assert!((w - 210.0 * PT_PER_MM).abs() < 0.01, "{w} pt wide");
        assert!((h - 148.0 * PT_PER_MM).abs() < 0.01, "{h} pt high");
    }
    // svg2pdf draws each page's SVG as a 1 pt square: it must be stretched over the whole page, or
    // nothing on it prints at scale.
    let pdf = String::from_utf8_lossy(&pdf);
    let draws: Vec<[f64; 6]> = pdf
        .match_indices(" cm\n/Page Do")
        .map(|(end, _)| {
            let line = &pdf[pdf[..end].rfind('\n').unwrap() + 1..end];
            let numbers: Vec<f64> = line.split(' ').map(|n| n.parse().unwrap()).collect();
            numbers.try_into().unwrap()
        })
        .collect();
    assert_eq!(draws.len(), 2, "each page drawn once");
    for [a, b, c, d, e, f] in draws {
        assert!((a - 210.0 * PT_PER_MM).abs() < 0.01 && (d - 148.0 * PT_PER_MM).abs() < 0.01);
        assert_eq!([b, c, e, f], [0.0; 4]);
    }
}
