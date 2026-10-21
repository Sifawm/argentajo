//! Nesting: a Job's pieces → a Layout on its Sheet, and a Layout → the printed page as an SVG.

use crate::piece::Piece;
use base64::{Engine, engine::general_purpose::STANDARD};
use jagua_rs::io::ext_repr::{ExtOrientation, ExtRotation, ExtSPolygon, ExtShape};
use jagua_rs::io::import::Importer;
use jagua_rs::probs::spp::io::ext_repr::{ExtItem, ExtSPInstance};
use jagua_rs::probs::spp::io::{export, import_instance};
use rand::SeedableRng;
use rand::rngs::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};
use sparrow::config::{DEFAULT_SPARROW_CONFIG, ShrinkDecayStrategy};
use sparrow::consts::{
    DEFAULT_COMPRESS_TIME_RATIO, DEFAULT_EXPLORE_TIME_RATIO, DEFAULT_FAIL_DECAY_RATIO_CMPR,
    DEFAULT_MAX_CONSEQ_FAILS_EXPL,
};
use sparrow::optimizer::lbf::ConstructionError;
use sparrow::optimizer::optimize;
use sparrow::util::listener::DummySolListener;
use sparrow::util::terminator::BasicTerminator;
use std::fmt::Write;
use std::time::Duration;

/// The printable page, oriented as printed. Sizes in mm.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    pub size_mm: [f64; 2],
    /// Border the printer can't print on.
    pub margin_mm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub paper: Paper,
    /// The silver Sheet, width × height in mm, as it lies on the paper.
    pub sheet_mm: [f64; 2],
    /// Minimum distance between pieces, for the saw.
    pub separation_mm: f64,
    /// Minimum distance between a piece and the Sheet's edge.
    pub edge_margin_mm: f64,
    pub items: Vec<JobItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JobItem {
    pub piece: Piece,
    pub quantity: u32,
}

/// The nesting result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// Top-left corner of the Sheet on the page, in mm.
    pub sheet_origin_mm: [f64; 2],
    /// One per placed Part.
    pub placements: Vec<Placement>,
    /// Copies placed, per Job item. A copy is placed only with all its Parts.
    pub placed: Vec<u32>,
    /// Copies that didn't fit, per Job item.
    pub missing: Vec<u32>,
    /// Area of the placed Parts over the Sheet's area.
    pub utilisation: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Placement {
    /// Index into `Job::items`.
    pub item: usize,
    /// Index into the Piece's `contours`.
    pub part: usize,
    /// Piece mm → Sheet mm, as an SVG `matrix(a b c d e f)`: x' = a·x + c·y + e, y' = b·x + d·y + f.
    pub transform: [f64; 6],
}

#[derive(Debug)]
pub enum NestError {
    /// A size isn't positive, or a separation or margin is negative.
    InvalidSettings,
    /// The Sheet doesn't fit on the paper inside the printer margin.
    SheetOffPaper,
    /// The nesting engine failed.
    Engine(String),
}

impl std::fmt::Display for NestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidSettings => write!(
                f,
                "sizes must be positive, and the separation and margins not negative"
            ),
            Self::SheetOffPaper => write!(
                f,
                "the sheet does not fit on the paper inside the printer margin"
            ),
            Self::Engine(e) => write!(f, "nesting failed: {e}"),
        }
    }
}

impl std::error::Error for NestError {}

/// Strip-packs the fixed quantities along the Sheet's long side with sparrow, compacted towards
/// its start. Copies past the Sheet's end, or that don't fit it at all, are missing.
pub fn nest(job: &Job, seed: u64, time_limit: Duration) -> Result<Layout, NestError> {
    let Paper {
        size_mm: [paper_w, paper_h],
        margin_mm,
    } = job.paper;
    let [w, h] = job.sheet_mm;
    let (separation, margin) = (job.separation_mm, job.edge_margin_mm);
    // Written so NaN fails too.
    if !(w > 0.0 && h > 0.0 && margin_mm >= 0.0 && separation >= 0.0 && margin >= 0.0) {
        return Err(NestError::InvalidSettings);
    }
    if w + 2.0 * margin_mm > paper_w || h + 2.0 * margin_mm > paper_h {
        return Err(NestError::SheetOffPaper);
    }

    // The strip runs along the Sheet's long side. A portrait Sheet is the strip turned a quarter;
    // items go in turned back a quarter, so they keep the orientation sparrow gives them.
    let portrait = h > w;
    let (long, short) = (w.max(h), w.min(h));
    // jagua-rs keeps items `separation` from the strip's sides: inset it so that is the edge margin.
    let inset = margin - separation;
    let (into_strip, to_sheet) = if portrait {
        (
            [0.0, -1.0, 1.0, 0.0, 0.0, 0.0],
            compose(
                translate(short, 0.0),
                compose([0.0, 1.0, -1.0, 0.0, 0.0, 0.0], translate(inset, inset)),
            ),
        )
    } else {
        (translate(0.0, 0.0), translate(inset, inset))
    };
    // Reflection axes are in the item's own coordinates: a left-right flip on the Sheet.
    let mirror_axis = if portrait { 0.0 } else { 90.0 };

    // One jagua-rs item per Part; its id indexes `parts`.
    let parts: Vec<(usize, usize)> = job
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.quantity > 0)
        .flat_map(|(i, item)| (0..item.piece.contours.len()).map(move |part| (i, part)))
        .collect();
    // Pieces with a Part too wide for the Sheet whichever way it may turn: all their copies are missing.
    let across = short - 2.0 * margin;
    let mut unplaceable: Vec<bool> = job
        .items
        .iter()
        .map(|item| item.piece.contours.is_empty())
        .collect();
    for &(i, part) in &parts {
        let piece = &job.items[i].piece;
        let contour: Vec<_> = piece.contours[part]
            .iter()
            .map(|&p| apply(into_strip, p))
            .collect();
        unplaceable[i] |= !fits_across(&contour, piece.allow_rotation, across);
    }
    let solution = loop {
        let items: Vec<ExtItem> = parts
            .iter()
            .enumerate()
            .filter(|(_, (i, _))| !unplaceable[*i])
            .map(|(id, &(i, part))| {
                let piece = &job.items[i].piece;
                ExtItem {
                    base: jagua_rs::io::ext_repr::ExtItem {
                        id: id as u64,
                        orientation: ExtOrientation {
                            // Not `Continuous`: sparrow then bounds the strip by area alone and, with
                            // few copies, tries strips shorter than a piece (its loss goes infinite).
                            rotation: if piece.allow_rotation {
                                ExtRotation::Stepped { step: 1.0 }
                            } else {
                                ExtRotation::Discrete { angles: vec![0.0] }
                            },
                            reflection_axes: if piece.allow_mirror {
                                vec![mirror_axis]
                            } else {
                                vec![]
                            },
                        },
                        shape: ExtShape::SimplePolygon(ExtSPolygon(
                            piece.contours[part]
                                .iter()
                                .map(|&p| {
                                    let [x, y] = apply(into_strip, p);
                                    (x as f32, y as f32)
                                })
                                .collect(),
                        )),
                        min_quality: None,
                    },
                    demand: job.items[i].quantity as u64,
                }
            })
            .collect();
        if items.is_empty() {
            break vec![];
        }
        let instance = ExtSPInstance {
            name: String::new(),
            min_item_separation: separation as f32,
            items,
            strip_height: (short - 2.0 * inset) as f32,
        };
        // jagua-rs asserts on some degenerate input; that must not take the caller down.
        let result = std::panic::catch_unwind(|| strip_pack(&instance, seed, time_limit))
            .map_err(|_| NestError::Engine("the nesting engine crashed".into()))?;
        match result {
            Ok(solution) => break solution,
            // A Part that only just fits across on paper, but not once jagua-rs has simplified and
            // inflated it: missing too.
            Err(e) => match e.downcast_ref::<ConstructionError>() {
                Some(&ConstructionError { external_id }) => {
                    unplaceable[parts[external_id as usize].0] = true
                }
                None => return Err(NestError::Engine(e.to_string())),
            },
        }
    };

    // Per Part, the placements that end before the Sheet's far edge margin, nearest the start first.
    // ponytail: copies past the end are dropped from the full-quantity solution, leaving their gaps;
    // re-run sparrow on what fits if those gaps ever matter.
    let end = long - margin - inset;
    let mut fitting = vec![vec![]; parts.len()];
    for (id, transform) in solution {
        let (i, part) = parts[id as usize];
        let in_strip = compose(transform, into_strip);
        let reach = job.items[i].piece.contours[part]
            .iter()
            .map(|&p| apply(in_strip, p)[0])
            .fold(f64::NEG_INFINITY, f64::max);
        if reach <= end {
            fitting[id as usize].push((reach, compose(to_sheet, in_strip)));
        }
    }
    let mut placed: Vec<u32> = job.items.iter().map(|item| item.quantity).collect();
    for (&(i, _), fits) in parts.iter().zip(&mut fitting) {
        placed[i] = placed[i].min(fits.len() as u32);
        fits.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    for (placed, &unplaceable) in placed.iter_mut().zip(&unplaceable) {
        if unplaceable {
            *placed = 0;
        }
    }

    let mut placements = vec![];
    let mut area = 0.0;
    for (&(item, part), fits) in parts.iter().zip(&fitting) {
        let n = placed[item] as usize;
        area += n as f64 * polygon_area(&job.items[item].piece.contours[part]);
        placements.extend(fits[..n].iter().map(|&(_, transform)| Placement {
            item,
            part,
            transform,
        }));
    }
    Ok(Layout {
        sheet_origin_mm: [margin_mm, margin_mm],
        placements,
        missing: job
            .items
            .iter()
            .zip(&placed)
            .map(|(item, placed)| item.quantity - placed)
            .collect(),
        placed,
        utilisation: area / (w * h),
    })
}

/// Whether `contour` fits in a strip `width` across, in some orientation it may take.
// ponytail: turns sampled every degree, with 0.01 mm to spare for jagua-rs' offsets; an exact fit
// between two samples counts as too wide.
fn fits_across(contour: &[[f64; 2]], allow_rotation: bool, width: f64) -> bool {
    let turns = if allow_rotation { 0..180 } else { 0..1 };
    turns.into_iter().any(|degrees| {
        let (sin, cos) = (degrees as f64).to_radians().sin_cos();
        let (min, max) = contour
            .iter()
            .map(|&[x, y]| sin * x + cos * y)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), y| {
                (min.min(y), max.max(y))
            });
        max - min <= width - 0.01
    })
}

/// sparrow's solution as (item id, transformation of the item's shape into the strip).
fn strip_pack(
    instance: &ExtSPInstance,
    seed: u64,
    time_limit: Duration,
) -> anyhow::Result<Vec<(u64, Affine)>> {
    let mut config = DEFAULT_SPARROW_CONFIG;
    config.expl_cfg.time_limit = time_limit.mul_f32(DEFAULT_EXPLORE_TIME_RATIO);
    config.cmpr_cfg.time_limit = time_limit.mul_f32(DEFAULT_COMPRESS_TIME_RATIO);
    // Stop once it stops improving, rather than always running out the clock.
    config.expl_cfg.max_conseq_failed_attempts = Some(DEFAULT_MAX_CONSEQ_FAILS_EXPL);
    config.cmpr_cfg.shrink_decay = ShrinkDecayStrategy::FailureBased(DEFAULT_FAIL_DECAY_RATIO_CMPR);
    let importer = Importer::new(
        config.cde_config,
        config.poly_simpl_tolerance,
        config.narrow_concavity_cutoff_ratio,
    );
    let solution = optimize(
        import_instance(&importer, instance)?,
        Xoshiro256PlusPlus::seed_from_u64(seed),
        &mut DummySolListener,
        &mut BasicTerminator::new(),
        &config.expl_cfg,
        &config.cmpr_cfg,
        None,
    )?;
    Ok(export(&solution, *sparrow::EPOCH)
        .layout
        .placed_items
        .into_iter()
        .map(|placed| {
            let t = placed.transformation;
            let (sin, cos) = (t.rotation as f64).to_radians().sin_cos();
            // Reflect (y negated), then rotate, then translate.
            let reflect = if t.reflected { -1.0 } else { 1.0 };
            let (x, y) = t.translation;
            (
                placed.item_id,
                [cos, sin, -sin * reflect, cos * reflect, x as f64, y as f64],
            )
        })
        .collect())
}

/// The page as an SVG in mm: the Sheet's outline and each placed Part's image, as nested.
pub fn render_page(job: &Job, layout: &Layout) -> String {
    let [page_w, page_h] = job.paper.size_mm;
    let [sheet_w, sheet_h] = job.sheet_mm;
    let [x, y] = layout.sheet_origin_mm;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{page_w}mm" height="{page_h}mm" viewBox="0 0 {page_w} {page_h}"><defs>"#
    );
    for (i, item) in job.items.iter().enumerate() {
        let [w, h] = item.piece.size_mm;
        _ = write!(
            svg,
            r#"<image id="piece{i}" width="{w}" height="{h}" preserveAspectRatio="none" href="data:image/png;base64,{}"/>"#,
            STANDARD.encode(&item.piece.png)
        );
        // Each Part is placed on its own: it shows only its share of the image.
        // ponytail: the contour runs ~0.5 px inside the motif's edge, so that rim is clipped off.
        if item.piece.contours.len() > 1 {
            for (part, contour) in item.piece.contours.iter().enumerate() {
                _ = write!(
                    svg,
                    r#"<clipPath id="part{i}-{part}"><polygon points="{}"/></clipPath>"#,
                    points(contour)
                );
            }
        }
    }
    _ = write!(
        svg,
        r#"</defs><rect width="{page_w}" height="{page_h}" fill="white"/><g transform="translate({x} {y})"><rect width="{sheet_w}" height="{sheet_h}" fill="none" stroke="black" stroke-width="0.2"/>"#
    );
    for p in &layout.placements {
        let [a, b, c, d, e, f] = p.transform;
        let clip = if job.items[p.item].piece.contours.len() > 1 {
            format!(r#" clip-path="url(#part{}-{})""#, p.item, p.part)
        } else {
            String::new()
        };
        // In a group, so the clip path is in the Piece's coordinates too.
        _ = write!(
            svg,
            r##"<g transform="matrix({a} {b} {c} {d} {e} {f})"><use href="#piece{}"{clip}/></g>"##,
            p.item
        );
    }
    svg + "</g></svg>"
}

fn points(polygon: &[[f64; 2]]) -> String {
    polygon
        .iter()
        .map(|[x, y]| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// An affine transformation in SVG `matrix(a b c d e f)` order.
type Affine = [f64; 6];

const fn translate(x: f64, y: f64) -> Affine {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

/// `m` after `n`.
fn compose(m: Affine, n: Affine) -> Affine {
    let [a1, b1, c1, d1, e1, f1] = m;
    let [a2, b2, c2, d2, e2, f2] = n;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

fn apply([a, b, c, d, e, f]: Affine, [x, y]: [f64; 2]) -> [f64; 2] {
    [a * x + c * y + e, b * x + d * y + f]
}

fn polygon_area(polygon: &[[f64; 2]]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| polygon[i][0] * polygon[(i + 1) % n][1] - polygon[(i + 1) % n][0] * polygon[i][1])
        .sum::<f64>()
        .abs()
        / 2.0
}
