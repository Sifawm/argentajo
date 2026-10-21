//! Nesting: a Job's pieces → a Layout on its Sheet, and a Layout → the printed page as an SVG.

use crate::piece::Piece;
use base64::{Engine, engine::general_purpose::STANDARD};
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
use sparrow::util::listener::{ReportType, SolutionListener};
use sparrow::util::terminator::{BasicTerminator, Terminator};
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The silver Sheet, width × height in mm, as it lies on the paper.
    pub sheet_mm: [f64; 2],
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
/// its start. Copies past the Sheet's end, or that don't fit it at all, are missing. Copies of
/// Fill items then go into the silver left over, as many as fit.
///
/// Runs until the Job's time limit or `cancel`, though always until a first Layout, and returns the
/// best Layout found: the most silver used by fixed quantities, then the shortest stretch of Sheet
/// they take. `on_progress` gets each Layout better than the last, so the last one it gets is the
/// one returned.
pub fn nest(
    job: &Job,
    seed: u64,
    mut on_progress: impl FnMut(&Layout),
    cancel: &AtomicBool,
) -> Result<Layout, NestError> {
    let deadline = Instant::now() + job.time_limit;
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
        .filter(|(_, item)| item.quantity != Quantity::Fixed(0))
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
        let mut placed: Vec<u32> = job.items.iter().map(|item| item.quantity.fixed()).collect();
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
        .filter(|&i| job.items[i].quantity == Quantity::Fill && !unplaceable[i])
        .collect();
    let filler = if fillable.is_empty() {
        None
    } else {
        let items = (0..parts.len())
            .filter(|&id| !unplaceable[parts[id].0])
            .map(|id| {
                let (i, part) = parts[id];
                let demand = match job.items[i].quantity {
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
    // at the sampled spot nearest the start of the Sheet where all its Parts fit, until none fits
    // or, unless it must `finish`, the run ends; and whether it got that far.
    // ponytail: greedy insertion, no copy is moved to make room for another; upgrade to a knapsack
    // optimiser on jagua-rs, or re-run sparrow with growing fill counts, if fill quality matters.
    let fill = |kept: Solution, finish: bool| -> (Solution, bool) {
        let Some((empty, fills)) = &filler else {
            return (kept, true);
        };
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

    let layout_of = |solution: &Solution| {
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
        Layout {
            sheet_origin_mm: [margin_mm, margin_mm],
            placements,
            missing: job
                .items
                .iter()
                .zip(&placed)
                .map(|(item, placed)| item.quantity.fixed().saturating_sub(*placed))
                .collect(),
            placed,
            utilisation: area / (w * h),
        }
    };

    // With the fixed quantities' share of the silver and length of Sheet.
    let mut best: Option<(Layout, f64, f64)> = None;
    loop {
        let items: Vec<ExtItem> = (0..parts.len())
            .filter(|&id| {
                let i = parts[id].0;
                !unplaceable[i] && job.items[i].quantity.fixed() > 0
            })
            .map(|id| ext_item(id, job.items[parts[id].0].quantity.fixed() as u64))
            .collect();
        if items.is_empty() {
            break;
        }
        let on_solution = |solution| {
            let (kept, length) = keep(&unplaceable, solution);
            let fixed = layout_of(&kept).utilisation;
            let better = best.as_ref().is_none_or(|(_, best_fixed, best_length)| {
                (fixed, -length) > (*best_fixed, -best_length)
            });
            if !better {
                return;
            }
            // The first Layout is always filled whole, like it is always waited for. A later fill
            // the run ended in the middle of can't stand in for a whole one.
            let (filled, whole) = fill(kept, best.is_none());
            if whole {
                let layout = layout_of(&filled);
                on_progress(&layout);
                best = Some((layout, fixed, length));
            }
        };
        match strip_pack(instance(items), seed, deadline, cancel, on_solution) {
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
        Some((layout, ..)) => layout,
        // No fixed quantities to place: only fill.
        None => {
            let layout = layout_of(&fill(vec![], true).0);
            on_progress(&layout);
            layout
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
/// narrower than the last, to `on_solution`.
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
            Ok(solution) => {
                solved = true;
                on_solution(solution);
            }
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

/// Sends on the exploration's and compression's feasible solutions, each narrower than the last.
struct Listener(mpsc::Sender<Solution>);

impl SolutionListener for Listener {
    fn report(&mut self, report: ReportType, solution: &SPSolution) {
        if !matches!(report, ReportType::ExplFeas | ReportType::CmprFeas) {
            return;
        }
        // Fails only once `strip_pack` has returned, and then nobody is listening.
        _ = self
            .0
            .send(export(solution, *sparrow::EPOCH).layout.placed_items);
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
