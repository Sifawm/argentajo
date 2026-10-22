//! Nesting: a Job's pieces → a Layout on its Sheets.

use crate::piece::Piece;
use jagua_rs::Instant;
use jagua_rs::geometry::DTransformation;
use jagua_rs::io::ext_repr::{
    ExtOrientation, ExtPlacedItem, ExtRotation, ExtSPolygon, ExtShape, ExtTransformation,
};
use jagua_rs::io::import::{Importer, ext_to_int_transformation};
use jagua_rs::probs::spp::entities::{SPPlacement, SPProblem, SPSolution};
use jagua_rs::probs::spp::io::ext_repr::{ExtItem, ExtSPInstance};
use jagua_rs::probs::spp::io::{export, import_instance};
use rand::SeedableRng;
use rand::rngs::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};
use sparrow::config::{DEFAULT_SPARROW_CONFIG, ShrinkDecayStrategy};
use sparrow::consts::{
    DEFAULT_COMPRESS_TIME_RATIO, DEFAULT_EXPLORE_TIME_RATIO, DEFAULT_FAIL_DECAY_RATIO_CMPR,
    DEFAULT_MAX_CONSEQ_FAILS_EXPL, LBF_SAMPLE_CONFIG,
};
use sparrow::eval::lbf_evaluator::LBFEvaluator;
use sparrow::eval::sample_eval::SampleEval;
use sparrow::optimizer::lbf::ConstructionError;
use sparrow::optimizer::optimize;
use sparrow::sample::search::search_placement;
use sparrow::util::listener::{ReportType, SeparationProgress, SolutionListener};
use sparrow::util::terminator::{BasicTerminator, Terminator};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
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
    /// The silver Sheets, width × height in mm as each lies on the paper, in the order they're filled.
    pub sheets_mm: Vec<[f64; 2]>,
    /// Minimum distance between pieces, for the saw.
    pub separation_mm: f64,
    /// Minimum distance between a piece and the Sheet's edge.
    pub edge_margin_mm: f64,
    /// Bound on a nesting run.
    pub time_limit: Duration,
    pub items: Vec<JobItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JobItem {
    pub piece: Piece,
    pub quantity: Quantity,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quantity {
    /// This many copies, or as many as fit, the rest missing.
    Fixed(u32),
    /// As many copies as fit in the silver the fixed quantities leave. Never missing.
    Fill,
}

impl Quantity {
    /// The copies that must be placed.
    fn fixed(self) -> u32 {
        match self {
            Self::Fixed(n) => n,
            Self::Fill => 0,
        }
    }
}

/// The nesting result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// One per Job Sheet.
    pub sheets: Vec<SheetLayout>,
    /// Copies placed, per Job item. A copy is placed only with all its Parts, on one Sheet.
    pub placed: Vec<u32>,
    /// Copies that didn't fit on any Sheet, per Job item.
    pub missing: Vec<u32>,
    /// Area of the placed Parts over the Sheets' area.
    pub utilisation: f64,
}

impl Layout {
    /// How many pages the Sheets take.
    pub fn pages(&self) -> usize {
        self.sheets
            .iter()
            .map(|sheet| sheet.page + 1)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetLayout {
    /// Index of the page it's on.
    pub page: usize,
    /// Top-left corner of the Sheet on its page, in mm.
    pub origin_mm: [f64; 2],
    /// One per Part placed on it.
    pub placements: Vec<Placement>,
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
    /// There is no Sheet, a size isn't positive, or a separation or margin is negative.
    InvalidSettings,
    /// A Sheet doesn't fit on the paper inside the printer margin, beside the verification square.
    SheetOffPaper,
    /// The nesting engine failed.
    Engine(String),
}

impl std::fmt::Display for NestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidSettings => write!(
                f,
                "there must be a sheet, sizes must be positive, and the separation and margins not negative"
            ),
            Self::SheetOffPaper => write!(
                f,
                "the sheet does not fit on the paper inside the printer margin, beside the 1 cm verification square"
            ),
            Self::Engine(e) => write!(f, "nesting failed: {e}"),
        }
    }
}

impl std::error::Error for NestError {}

/// Between Sheets on the page, so the paper can be cut apart per Sheet.
const SHEET_GAP_MM: f64 = 5.0;

/// Side of the square on every page that checks the print's scale with a ruler.
pub(crate) const VERIFICATION_MM: f64 = 10.0;

/// Top-left corner of the verification square: the bottom right of the printable area, where the
/// Sheets, placed from the top left, leave room for it.
pub(crate) fn verification_square(paper: Paper) -> [f64; 2] {
    let [w, h] = paper.size_mm;
    let m = paper.margin_mm;
    [w - m - VERIFICATION_MM, h - m - VERIFICATION_MM]
}

/// Nests the Sheets in order, each taking what it can of the fixed quantities the ones before it
/// left; copies no Sheet takes are missing. Fill items go into every Sheet's leftover silver. The
/// Sheets go on the paper in rows from the top left, a new page when one doesn't fit.
///
/// Runs until the Job's time limit or `cancel`, though always until a first Layout of every Sheet,
/// and returns the best Layout found. `on_progress` gets each Layout better than the last, so the
/// last one it gets is the one returned. `iterations` counts the attempts behind them: sparrow's and
/// each fill.
pub fn nest(
    job: &Job,
    seed: u64,
    mut on_progress: impl FnMut(&Layout),
    cancel: &AtomicBool,
    iterations: &AtomicU64,
) -> Result<Layout, NestError> {
    let deadline = Instant::now() + job.time_limit;
    let sheets = &job.sheets_mm;
    // Written so NaN fails too.
    let valid = !sheets.is_empty()
        && sheets.iter().all(|&[w, h]| w > 0.0 && h > 0.0)
        && job.paper.margin_mm >= 0.0
        && job.separation_mm >= 0.0
        && job.edge_margin_mm >= 0.0;
    if !valid {
        return Err(NestError::InvalidSettings);
    }
    let mut layout = Layout {
        sheets: place_sheets(job.paper, sheets)?,
        placed: vec![0; job.items.len()],
        missing: job.items.iter().map(|item| item.quantity.fixed()).collect(),
        utilisation: 0.0,
    };
    let silver: f64 = sheets.iter().map(|[w, h]| w * h).sum();
    let mut area = 0.0;
    // Still to place.
    let mut quantities: Vec<Quantity> = job.items.iter().map(|item| item.quantity).collect();
    // Each Sheet keeps its best before the next starts, with an equal share of the time left among
    // the Sheets the fixed copies still to place need, or, once there are none, all those left for
    // fill. Time a Sheet doesn't use goes to the next.
    // ponytail: the Sheets needed are counted by area, as if the copies packed with no gaps, so a
    // Sheet the copies overflow onto may get only its first Layout; estimate with the density
    // sparrow reaches if those Sheets come out loose.
    for (s, &sheet) in sheets.iter().enumerate() {
        let mut to_place: f64 = job
            .items
            .iter()
            .zip(&quantities)
            .map(|(item, quantity)| {
                let area: f64 = item.piece.contours.iter().map(|c| polygon_area(c)).sum();
                quantity.fixed() as f64 * area
            })
            .sum();
        let mut needed = 0;
        for &[w, h] in &sheets[s..] {
            if to_place <= 0.0 {
                break;
            }
            let usable =
                (w - 2.0 * job.edge_margin_mm).max(0.0) * (h - 2.0 * job.edge_margin_mm).max(0.0);
            to_place -= usable;
            needed += 1;
        }
        if needed == 0 {
            needed = sheets.len() - s;
        }
        let now = Instant::now();
        let sheet_deadline = now + deadline.saturating_duration_since(now) / needed as u32;
        let with = |nested: &Nested| {
            let mut next = layout.clone();
            next.sheets[s].placements = nested.placements.clone();
            for (i, &placed) in nested.placed.iter().enumerate() {
                next.placed[i] += placed;
                next.missing[i] = next.missing[i].saturating_sub(placed);
            }
            next.utilisation = (area + nested.area) / silver;
            next
        };
        let nested = nest_sheet(
            job,
            sheet,
            &quantities,
            seed,
            sheet_deadline,
            cancel,
            iterations,
            // An empty later Sheet changes nothing.
            |nested| {
                if s == 0 || !nested.placements.is_empty() {
                    on_progress(&with(nested))
                }
            },
        )?;
        layout = with(&nested);
        area += nested.area;
        for (quantity, &placed) in quantities.iter_mut().zip(&nested.placed) {
            if let Quantity::Fixed(n) = quantity {
                *n -= placed;
            }
        }
    }
    Ok(layout)
}

/// The Sheets on the pages, still empty: in rows from the top left inside the printer margin,
/// `SHEET_GAP_MM` apart and from the verification square, starting a new row when one doesn't fit
/// in this one and a new page when it doesn't fit in the next.
fn place_sheets(paper: Paper, sheets: &[[f64; 2]]) -> Result<Vec<SheetLayout>, NestError> {
    let Paper {
        size_mm: [paper_w, paper_h],
        margin_mm: m,
    } = paper;
    let [square_x, square_y] = verification_square(paper);
    let (mut page, mut x, mut y, mut row_h) = (0, m, m, 0.0f64);
    sheets
        .iter()
        .map(|&[w, h]| {
            let fits = |x: f64, y: f64| {
                x + w <= paper_w - m
                    && y + h <= paper_h - m
                    && (x + w <= square_x - SHEET_GAP_MM || y + h <= square_y - SHEET_GAP_MM)
            };
            if !fits(x, y) {
                (x, y, row_h) = (m, y + row_h + SHEET_GAP_MM, 0.0);
            }
            if !fits(x, y) {
                (page, x, y, row_h) = (page + 1, m, m, 0.0);
            }
            if !fits(x, y) {
                return Err(NestError::SheetOffPaper);
            }
            let sheet = SheetLayout {
                page,
                origin_mm: [x, y],
                placements: vec![],
            };
            x += w + SHEET_GAP_MM;
            row_h = row_h.max(h);
            Ok(sheet)
        })
        .collect()
}

/// What one Sheet holds.
struct Nested {
    placements: Vec<Placement>,
    /// Copies placed, per Job item.
    placed: Vec<u32>,
    /// Of the placed Parts, in mm².
    area: f64,
}

/// Strip-packs `quantities`' fixed copies along the Sheet's long side with sparrow, compacted
/// towards its start. Copies past the Sheet's end, or that don't fit it at all, are left out.
/// Copies of Fill items then go into the silver left over, as many as fit.
///
/// Runs until `deadline` or `cancel`, though always until a first result, and returns the best
/// found: the most silver used by fixed quantities, then the most used in all, then the shortest
/// stretch of Sheet the fixed quantities take. `on_progress` gets each result better than the
/// last, so the last one it gets is the one returned.
#[allow(clippy::too_many_arguments)]
fn nest_sheet(
    job: &Job,
    [w, h]: [f64; 2],
    quantities: &[Quantity],
    seed: u64,
    deadline: Instant,
    cancel: &AtomicBool,
    iterations: &AtomicU64,
    mut on_progress: impl FnMut(&Nested),
) -> Result<Nested, NestError> {
    let (separation, margin) = (job.separation_mm, job.edge_margin_mm);

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
        .filter(|&(i, _)| quantities[i] != Quantity::Fixed(0))
        .flat_map(|(i, item)| (0..item.piece.contours.len()).map(move |part| (i, part)))
        .collect();
    // Pieces with a Part too wide for the Sheet whichever way it may turn: none of their copies go on it.
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
    let ext_item = |id: usize, demand: u64| {
        let (i, part) = parts[id];
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
            demand,
        }
    };
    let instance = |items| ExtSPInstance {
        name: String::new(),
        min_item_separation: separation as f32,
        items,
        strip_height: (short - 2.0 * inset) as f32,
    };
    // In the strip, the end of the Sheet's far edge margin.
    let end = long - margin - inset;

    // Per Part, the placements that end before the Sheet's far edge margin, nearest the start first,
    // with the length of Sheet they use.
    // ponytail: copies past the end are dropped from the full-quantity solution, leaving their gaps
    // for fill only; re-run sparrow on what fits if those gaps ever matter.
    let keep = |unplaceable: &[bool], solution: Solution| {
        let mut fitting = vec![vec![]; parts.len()];
        for placed in solution {
            let (i, part) = parts[placed.item_id as usize];
            let in_strip = compose(affine(&placed.transformation), into_strip);
            let reach = job.items[i].piece.contours[part]
                .iter()
                .map(|&p| apply(in_strip, p)[0])
                .fold(f64::NEG_INFINITY, f64::max);
            if reach <= end {
                fitting[placed.item_id as usize].push((reach, placed));
            }
        }
        let mut placed: Vec<u32> = quantities.iter().map(|q| q.fixed()).collect();
        for (&(i, _), fits) in parts.iter().zip(&mut fitting) {
            placed[i] = placed[i].min(fits.len() as u32);
            fits.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let mut kept = vec![];
        let mut length = 0.0f64;
        for (&(i, _), fits) in parts.iter().zip(fitting) {
            let n = if unplaceable[i] {
                0
            } else {
                placed[i] as usize
            };
            for (reach, placed) in fits.into_iter().take(n) {
                length = length.max(reach);
                kept.push(placed);
            }
        }
        (kept, length)
    };

    // The Fill items' Parts with the fixed ones, on the whole usable Sheet, for `fill`; and per
    // Fill item, its Parts' indices there.
    let fillable: Vec<usize> = (0..job.items.len())
        .filter(|&i| quantities[i] == Quantity::Fill && !unplaceable[i])
        .collect();
    let filler = if fillable.is_empty() {
        None
    } else {
        let items = (0..parts.len())
            .filter(|&id| !unplaceable[parts[id].0])
            .map(|id| {
                let (i, part) = parts[id];
                let demand = match quantities[i] {
                    Quantity::Fixed(n) => n as u64,
                    // More copies than could ever fit.
                    // ponytail: capped at 10,001 copies against a degenerate zero-area Part; lift
                    // the cap if pieces ever get that small.
                    Quantity::Fill => {
                        (w * h / polygon_area(&job.items[i].piece.contours[part])).min(1e4) as u64
                            + 1
                    }
                };
                ext_item(id, demand)
            })
            .collect();
        let engine = |e: anyhow::Error| NestError::Engine(e.to_string());
        let config = DEFAULT_SPARROW_CONFIG;
        let importer = Importer::new(
            config.cde_config,
            config.poly_simpl_tolerance,
            config.narrow_concavity_cutoff_ratio,
        );
        let mut empty =
            SPProblem::new(import_instance(&importer, &instance(items)).map_err(engine)?)
                .map_err(engine)?;
        empty
            .change_strip_width((long - 2.0 * inset) as f32)
            .map_err(engine)?;
        let item_idx = |id: usize| empty.instance().item_idx(id as u64).unwrap();
        let fills: Vec<Vec<usize>> = fillable
            .iter()
            .map(|&i| {
                (0..parts.len())
                    .filter(|&id| parts[id].0 == i)
                    .map(item_idx)
                    .collect()
            })
            .collect();
        Some((empty, fills))
    };
    // `kept` with copies of the Fill items inserted one by one into the free space, in turn, each
    // at the spot nearest the start of the Sheet where all its Parts fit, of those sampled with
    // `seed`, until none fits or, unless it must `finish`, the run ends; and whether it got that far.
    // ponytail: greedy insertion, no copy is moved to make room for another; upgrade to a knapsack
    // optimiser on jagua-rs, or re-run sparrow with growing fill counts, if fill quality matters.
    let fill = |kept: Solution, finish: bool, seed: u64| -> (Solution, bool) {
        let Some((empty, fills)) = &filler else {
            return (kept, true);
        };
        iterations.fetch_add(1, Ordering::Relaxed);
        let mut prob = empty.clone();
        let instance = empty.instance();
        for placed in kept {
            let item_idx = instance
                .item_idx(placed.item_id)
                .expect("kept parts are placeable");
            let d_transf = ext_to_int_transformation(
                &DTransformation::from(placed.transformation),
                &instance.item(item_idx).shape_orig().pre_transform,
            );
            prob.place_item(SPPlacement { item_idx, d_transf });
        }
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
        let mut active: Vec<&[usize]> = fills.iter().map(Vec::as_slice).collect();
        let ended = || !finish && (cancel.load(Ordering::Relaxed) || Instant::now() > deadline);
        while !active.is_empty() && !ended() {
            active.retain(|copy| {
                if ended() {
                    return true;
                }
                let mut placed = vec![];
                for &item_idx in *copy {
                    let item = instance.item(item_idx);
                    let evaluator = LBFEvaluator::new(prob.layout(), item);
                    let (sample, _) = search_placement(
                        prob.layout(),
                        item,
                        None,
                        evaluator,
                        LBF_SAMPLE_CONFIG,
                        &mut rng,
                    );
                    match sample {
                        Some((d_transf, SampleEval::Clear { .. }))
                            if prob.item_demand_qtys()[item_idx] > 0 =>
                        {
                            placed.push(prob.place_item(SPPlacement { item_idx, d_transf }))
                        }
                        // All its Parts or none.
                        _ => {
                            for key in placed {
                                prob.remove_item(key);
                            }
                            return false;
                        }
                    }
                }
                true
            });
        }
        (
            export(&prob.save(), *sparrow::EPOCH).layout.placed_items,
            active.is_empty(),
        )
    };

    let nested_of = |solution: &Solution| {
        let mut counts = vec![0u32; parts.len()];
        let mut area = 0.0;
        let placements = solution
            .iter()
            .map(|placed| {
                let id = placed.item_id as usize;
                let (item, part) = parts[id];
                counts[id] += 1;
                area += polygon_area(&job.items[item].piece.contours[part]);
                Placement {
                    item,
                    part,
                    transform: compose(
                        to_sheet,
                        compose(affine(&placed.transformation), into_strip),
                    ),
                }
            })
            .collect();
        // A copy is placed with all its Parts.
        let placed: Vec<u32> = (0..job.items.len())
            .map(|i| {
                parts
                    .iter()
                    .zip(&counts)
                    .filter(|((item, _), _)| *item == i)
                    .map(|(_, &n)| n)
                    .min()
                    .unwrap_or(0)
            })
            .collect();
        Nested {
            placements,
            placed,
            area,
        }
    };
    // Of this Sheet.
    let share = |nested: &Nested| nested.area / (w * h);

    // With the fixed quantities' share of the silver and length of Sheet.
    let mut best: Option<(Nested, f64, f64)> = None;
    loop {
        let items: Vec<ExtItem> = (0..parts.len())
            .filter(|&id| {
                let i = parts[id].0;
                !unplaceable[i] && quantities[i].fixed() > 0
            })
            .map(|id| ext_item(id, quantities[parts[id].0].fixed() as u64))
            .collect();
        if items.is_empty() {
            break;
        }
        // The same copies' areas summed in another order can differ in their last bits.
        let same = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let on_solution = |solution| {
            let (kept, length) = keep(&unplaceable, solution);
            let fixed = share(&nested_of(&kept));
            if best
                .as_ref()
                .is_some_and(|(_, best_fixed, _)| fixed < *best_fixed && !same(fixed, *best_fixed))
            {
                return;
            }
            // The first Layout is always filled whole, like it is always waited for. A later fill
            // the run ended in the middle of can't stand in for a whole one.
            let (filled, whole) = fill(kept, best.is_none(), seed);
            if !whole {
                return;
            }
            // A shorter stretch of fixed quantities can leave fill less room: compare once filled.
            let nested = nested_of(&filled);
            let better = best.as_ref().is_none_or(|(best, best_fixed, best_length)| {
                if !same(fixed, *best_fixed) {
                    fixed > *best_fixed
                } else if !same(share(&nested), share(best)) {
                    share(&nested) > share(best)
                } else {
                    length < *best_length
                }
            });
            if better {
                on_progress(&nested);
                best = Some((nested, fixed, length));
            }
        };
        match strip_pack(
            instance(items),
            seed,
            deadline,
            cancel,
            iterations,
            on_solution,
        ) {
            Ok(()) => break,
            // A Part that only just fits across on paper, but not once jagua-rs has simplified and
            // inflated it: missing too.
            Err(e) => match e.downcast_ref::<ConstructionError>() {
                Some(&ConstructionError { external_id }) => {
                    unplaceable[parts[external_id as usize].0] = true
                }
                None => return Err(NestError::Engine(e.to_string())),
            },
        }
    }
    Ok(match best {
        Some((nested, ..)) => nested,
        // No fixed quantities to place: only fill, retried with the next seeds until the run ends.
        // ponytail: random restarts of the greedy fill; search around the best one if they plateau.
        None => {
            let mut best = nested_of(&fill(vec![], true, seed).0);
            on_progress(&best);
            if filler.is_some() {
                for seed in (1..).map(|k| seed.wrapping_add(k)) {
                    let (filled, whole) = fill(vec![], false, seed);
                    if !whole {
                        break;
                    }
                    let nested = nested_of(&filled);
                    if share(&nested) > share(&best) + 1e-9 {
                        on_progress(&nested);
                        best = nested;
                    }
                }
            }
            best
        }
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

/// Runs sparrow until `deadline` or `cancel`, passing each feasible solution it reaches, every one
/// narrower than the last, to `on_solution`, and counting its iterations in `iterations`.
///
/// sparrow runs on its own thread: it checks for the end only between steps, and some steps take
/// seconds on a crowded strip. This returns at the deadline or on cancel all the same, once there
/// is a first solution, and leaves sparrow to end at its next check.
// ponytail: sparrow keeps a core busy for that last step after we return; patch it to check more
// often if overlapping runs ever matter.
fn strip_pack(
    instance: ExtSPInstance,
    seed: u64,
    deadline: Instant,
    cancel: &AtomicBool,
    iterations: &AtomicU64,
    mut on_solution: impl FnMut(Solution),
) -> anyhow::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let (send, solutions) = mpsc::channel();
    let sparrow = {
        let stop = stop.clone();
        thread::spawn(move || -> anyhow::Result<()> {
            let time_limit = deadline.saturating_duration_since(Instant::now());
            let mut config = DEFAULT_SPARROW_CONFIG;
            config.expl_cfg.time_limit = time_limit.mul_f32(DEFAULT_EXPLORE_TIME_RATIO);
            config.cmpr_cfg.time_limit = time_limit.mul_f32(DEFAULT_COMPRESS_TIME_RATIO);
            // Stop once it stops improving, rather than always running out the clock.
            config.expl_cfg.max_conseq_failed_attempts = Some(DEFAULT_MAX_CONSEQ_FAILS_EXPL);
            config.cmpr_cfg.shrink_decay =
                ShrinkDecayStrategy::FailureBased(DEFAULT_FAIL_DECAY_RATIO_CMPR);
            let importer = Importer::new(
                config.cde_config,
                config.poly_simpl_tolerance,
                config.narrow_concavity_cutoff_ratio,
            );
            optimize(
                import_instance(&importer, &instance)?,
                Xoshiro256PlusPlus::seed_from_u64(seed),
                &mut Listener(send),
                &mut Stopper {
                    phase: BasicTerminator::new(),
                    deadline,
                    stop,
                },
                &config.expl_cfg,
                &config.cmpr_cfg,
                None,
            )?;
            Ok(())
        })
    };
    let mut solved = false;
    loop {
        // Wakes this often to notice the deadline or a cancel between solutions.
        match solutions.recv_timeout(Duration::from_millis(20)) {
            Ok(Report::Solution(solution)) => {
                solved = true;
                on_solution(solution);
            }
            Ok(Report::Iteration) => _ = iterations.fetch_add(1, Ordering::Relaxed),
            Err(RecvTimeoutError::Timeout) => {}
            // jagua-rs asserts on some degenerate input (10 mm squares, seed 2); that must not take
            // the caller down, nor lose the solutions so far.
            // ponytail: a crash after a first solution ends the run quietly with the best so far;
            // report it if early ends ever need explaining.
            Err(RecvTimeoutError::Disconnected) => {
                return match sparrow.join() {
                    Ok(result) => result,
                    Err(_) if solved => Ok(()),
                    Err(_) => Err(anyhow::anyhow!("the nesting engine crashed")),
                };
            }
        }
        if solved && (cancel.load(Ordering::Relaxed) || Instant::now() > deadline) {
            stop.store(true, Ordering::Relaxed);
            return Ok(());
        }
    }
}

enum Report {
    Solution(Solution),
    Iteration,
}

/// Sends on the exploration's and compression's feasible solutions, each narrower than the last,
/// and each iteration of sparrow's separator.
// Sends fail only once `strip_pack` has returned, and then nobody is listening.
struct Listener(mpsc::Sender<Report>);

impl SolutionListener for Listener {
    fn report(&mut self, report: ReportType, solution: &SPSolution) {
        if !matches!(report, ReportType::ExplFeas | ReportType::CmprFeas) {
            return;
        }
        _ = self.0.send(Report::Solution(
            export(solution, *sparrow::EPOCH).layout.placed_items,
        ));
    }

    // Iteration zero is the layout before any.
    fn report_separation_progress(&mut self, progress: SeparationProgress) {
        if progress.iteration > 0 {
            _ = self.0.send(Report::Iteration);
        }
    }
}

/// Ends each of sparrow's phases at its own time limit, and the whole run at the deadline or once
/// it's no longer wanted.
struct Stopper {
    phase: BasicTerminator,
    deadline: Instant,
    stop: Arc<AtomicBool>,
}

impl Terminator for Stopper {
    fn kill(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || Instant::now() > self.deadline || self.phase.kill()
    }

    fn new_timeout(&mut self, timeout: Duration) {
        self.phase.new_timeout(timeout)
    }

    fn timeout_at(&self) -> Option<Instant> {
        self.phase.timeout_at().map(|t| t.min(self.deadline))
    }
}

/// An affine transformation in SVG `matrix(a b c d e f)` order.
type Affine = [f64; 6];

/// A jagua-rs solution: per placed item, its id and the transformation of its shape into the strip.
type Solution = Vec<ExtPlacedItem>;

fn affine(t: &ExtTransformation) -> Affine {
    let (sin, cos) = (t.rotation as f64).to_radians().sin_cos();
    // Reflect (y negated), then rotate, then translate.
    let reflect = if t.reflected { -1.0 } else { 1.0 };
    let (x, y) = t.translation;
    [cos, sin, -sin * reflect, cos * reflect, x as f64, y as f64]
}

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
