//! Job files: save → open gives back the same Job, Layout and output options, and anything else is
//! refused rather than opened.

use argentajo_core::job_file::{SavedJob, open_job, save_job};
use argentajo_core::nest::Quantity::{Fill, Fixed};
use argentajo_core::nest::{Job, JobItem, Paper, nest};
use argentajo_core::output::OutputOptions;
use argentajo_core::piece::{NORMAL_SIZE_ID, PrepareOptions, RealSize, SizeInput, prepare_piece};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Two real Pieces, one of them in two Parts, nested on two Sheets.
fn saved() -> SavedJob {
    let prepare = |name, width| {
        prepare_piece(
            &fixture(name),
            &PrepareOptions {
                sizes: vec![SizeInput {
                    id: NORMAL_SIZE_ID.into(),
                    name: "Normal".into(),
                    measure: Some(RealSize::Width(width)),
                }],
                ..Default::default()
            },
        )
        .unwrap()
    };
    let job = Job {
        paper: Paper {
            size_mm: [297.0, 210.0],
            margin_mm: 5.0,
        },
        sheets_mm: vec![[80.0, 50.0], [60.0, 40.0]],
        separation_mm: 1.0,
        edge_margin_mm: 1.5,
        time_limit: Duration::from_millis(1500),
        items: vec![
            JobItem {
                piece: prepare("ring.png", 18.3),
                size_id: NORMAL_SIZE_ID.into(),
                quantity: Fixed(3),
            },
            JobItem {
                piece: prepare("two_parts.png", 21.7),
                size_id: NORMAL_SIZE_ID.into(),
                quantity: Fill,
            },
        ],
    };
    // Cancelled from the start: the first Layout of every Sheet.
    let layout = nest(&job, 0, |_| {}, &AtomicBool::new(true), &AtomicU64::new(0)).unwrap();
    assert!(layout.placed.iter().sum::<u32>() > 0);
    SavedJob {
        job,
        layout: Some(layout),
        output: OutputOptions {
            sheet_outline: false,
            silhouette: true,
        },
    }
}

#[test]
fn save_then_open_gives_back_the_same_job_and_layout() {
    let saved = saved();
    assert!(
        saved
            .job
            .items
            .iter()
            .all(|item| !item.piece.png.is_empty())
    );
    assert_eq!(open_job(save_job(&saved).as_bytes()).unwrap(), saved);
}

#[test]
fn a_job_saved_before_nesting_opens_without_a_layout() {
    let saved = SavedJob {
        layout: None,
        ..saved()
    };
    assert_eq!(open_job(save_job(&saved).as_bytes()).unwrap(), saved);
}

#[test]
fn what_is_not_a_job_file_is_refused() {
    for bytes in [&b""[..], b"\x89PNG\r\n", b"{}", b"[1, 2]", b"{\"job\": 3}"] {
        assert!(
            open_job(bytes).is_err(),
            "{}",
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn a_truncated_file_is_refused() {
    let json = save_job(&saved());
    assert!(open_job(&json.as_bytes()[..json.len() / 2]).is_err());
}

#[test]
fn a_piece_image_that_is_not_base64_is_refused() {
    let json = save_job(&saved()).replacen(r#""png": ""#, r#""png": "%%"#, 1);
    assert!(open_job(json.as_bytes()).is_err());
}

#[test]
fn a_layout_that_does_not_match_its_job_is_refused() {
    let corruptions: [fn(&mut SavedJob); 6] = [
        |saved| saved.job.sheets_mm.pop().map(drop).unwrap(),
        // More pages than Sheets: rendering them all would never end.
        |saved| saved.layout.as_mut().unwrap().sheets[0].page = 2,
        |saved| saved.job.items.pop().map(drop).unwrap(),
        |saved| saved.layout.as_mut().unwrap().placed.push(0),
        |saved| placement(saved).item = 2,
        |saved| placement(saved).part = 2,
    ];
    for corrupt in corruptions {
        let mut saved = saved();
        corrupt(&mut saved);
        assert!(open_job(save_job(&saved).as_bytes()).is_err());
    }
}

fn placement(saved: &mut SavedJob) -> &mut argentajo_core::nest::Placement {
    let layout = saved.layout.as_mut().unwrap();
    layout
        .sheets
        .iter_mut()
        .flat_map(|sheet| &mut sheet.placements)
        .next()
        .unwrap()
}

#[test]
fn a_job_with_pieces_at_different_sizes_round_trips() {
    let piece = prepare_piece(
        &fixture("flower_on_white.png"),
        &PrepareOptions {
            sizes: vec![
                SizeInput {
                    id: "small".into(),
                    name: "Small".into(),
                    measure: Some(RealSize::Width(20.0)),
                },
                SizeInput {
                    id: "large".into(),
                    name: "Large".into(),
                    measure: Some(RealSize::Width(40.0)),
                },
            ],
            ..Default::default()
        },
    )
    .unwrap();
    let job = Job {
        paper: Paper {
            size_mm: [297.0, 210.0],
            margin_mm: 5.0,
        },
        sheets_mm: vec![[80.0, 50.0]],
        separation_mm: 1.0,
        edge_margin_mm: 1.5,
        time_limit: Duration::from_millis(500),
        items: vec![
            JobItem {
                piece: piece.clone(),
                size_id: "small".into(),
                quantity: Fixed(1),
            },
            JobItem {
                piece: piece.clone(),
                size_id: "large".into(),
                quantity: Fixed(1),
            },
        ],
    };
    let saved = SavedJob {
        job,
        layout: None,
        output: OutputOptions::default(),
    };
    assert_eq!(open_job(save_job(&saved).as_bytes()).unwrap(), saved);
}

#[test]
fn an_old_job_file_without_sizes_opens_as_normal() {
    let saved = saved();
    let mut value = serde_json::to_value(&saved).unwrap();
    for item in value["job"]["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("sizeId");
        item["piece"].as_object_mut().unwrap().remove("sizes");
    }
    let opened = open_job(serde_json::to_vec(&value).unwrap().as_slice()).unwrap();
    for item in &opened.job.items {
        assert_eq!(item.size_id, NORMAL_SIZE_ID);
        assert_eq!(item.piece.sizes.len(), 1);
        assert_eq!(item.piece.sizes[0].id, NORMAL_SIZE_ID);
        assert_eq!(item.piece.sizes[0].name, "Normal");
    }
}
