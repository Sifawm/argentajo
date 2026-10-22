// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argentajo_core::nest::{Job, JobItem, Layout, Paper, Quantity, nest};
use argentajo_core::output::{OutputOptions, export_pdf, render_page};
use argentajo_core::piece::{Piece, PrepareOptions, prepare_piece, split_piece};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
fn core_version() -> &'static str {
    argentajo_core::version()
}

/// A Piece as the frontend sees it: the cleaned PNG as a data URL next to its metadata.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PieceView {
    id: Option<String>,
    image: String,
    #[serde(flatten)]
    piece: Piece,
}

impl PieceView {
    fn new(id: Option<String>, piece: Piece) -> Self {
        let image = format!("data:image/png;base64,{}", STANDARD.encode(&piece.png));
        Self { id, image, piece }
    }
}

/// Raw image bytes as the body, `PrepareOptions` as JSON in the `options` header,
/// so the slider can re-prepare without serialising the image as a JSON array.
/// Manual edits, if any, are appended to the body; `edits-length` says how many bytes they take.
fn parse<'a>(request: &'a Request<'_>) -> Result<(&'a [u8], PrepareOptions), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the image bytes as the request body".into());
    };
    let mut options: PrepareOptions =
        serde_json::from_slice(header(request, "options").ok_or("missing options header")?)
            .map_err(|e| e.to_string())?;
    let edits_length = match header(request, "edits-length") {
        Some(length) => std::str::from_utf8(length)
            .ok()
            .and_then(|length| length.parse().ok())
            .filter(|&length| length <= bytes.len())
            .ok_or("invalid edits-length header")?,
        None => 0,
    };
    let (image, edits) = bytes.split_at(bytes.len() - edits_length);
    options.edits = (edits_length > 0).then(|| edits.to_vec());
    Ok((image, options))
}

fn header<'a>(request: &'a Request<'_>, name: &str) -> Option<&'a [u8]> {
    request.headers().get(name).map(|v| v.as_bytes())
}

/// A header holding text, such as the image's MIME type in `type` or the piece's `id`.
fn text_header<'a>(request: &'a Request<'_>, name: &str) -> Result<&'a str, String> {
    header(request, name)
        .and_then(|v| std::str::from_utf8(v).ok())
        .ok_or_else(|| format!("missing {name} header"))
}

// `async`: sync commands run on the main thread and would freeze the window while preparing.
#[tauri::command(async)]
fn preview_piece(request: Request) -> Result<PieceView, String> {
    let (image, options) = parse(&request)?;
    Ok(PieceView::new(
        None,
        prepare_piece(image, &options).map_err(|e| e.to_string())?,
    ))
}

fn library_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("library"))
}

/// The folder of the Library piece `id`. Ids are timestamps: anything else could escape the Library.
fn piece_dir(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid piece id: {id}"));
    }
    Ok(library_dir(app)?.join(id))
}

/// A new, empty piece folder named by a fresh id.
fn new_piece_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let library = library_dir(app)?;
    fs::create_dir_all(&library).map_err(|e| e.to_string())?;
    // A split saves several in a row: on a coarse clock the next id may still be taken.
    loop {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let dir = library.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// How the editor reopens a Piece: the image it was prepared from, its MIME type and its options.
#[derive(Serialize, Deserialize)]
struct Source {
    #[serde(rename = "type")]
    mime: String,
    options: PrepareOptions,
}

/// Library layout: one folder per Piece holding `piece.png` and `piece.json`, plus what the editor
/// reopens it from: `source` with `source.json`, and `edits.png` when it was brushed. Without them
/// (split pieces) it reopens from `piece.png`.
fn save(dir: PathBuf, piece: Piece, source: Option<(&[u8], Source)>) -> Result<PieceView, String> {
    let write =
        |name: &str, bytes: &[u8]| fs::write(dir.join(name), bytes).map_err(|e| e.to_string());
    write("piece.png", &piece.png)?;
    write("piece.json", &serde_json::to_vec_pretty(&piece).unwrap())?;
    if let Some((image, source)) = source {
        write("source", image)?;
        write("source.json", &serde_json::to_vec_pretty(&source).unwrap())?;
        match &source.options.edits {
            Some(edits) => write("edits.png", edits)?,
            None => _ = fs::remove_file(dir.join("edits.png")),
        }
    }
    let id = dir.file_name().unwrap().to_string_lossy().into_owned();
    Ok(PieceView::new(Some(id), piece))
}

/// Prepares the request's image and saves it, with its source, into `dir`.
fn prepare_into(dir: PathBuf, request: &Request) -> Result<PieceView, String> {
    let (image, options) = parse(request)?;
    let piece = prepare_piece(image, &options).map_err(|e| e.to_string())?;
    let mime = text_header(request, "type")?.to_owned();
    save(dir, piece, Some((image, Source { mime, options })))
}

#[tauri::command(async)]
fn add_to_library(app: tauri::AppHandle, request: Request) -> Result<PieceView, String> {
    prepare_into(new_piece_dir(&app)?, &request)
}

/// Re-prepares the Library piece in the `id` header and overwrites it in place.
#[tauri::command(async)]
fn update_piece(app: tauri::AppHandle, request: Request) -> Result<PieceView, String> {
    let dir = piece_dir(&app, text_header(&request, "id")?)?;
    if !dir.is_dir() {
        return Err("this piece is no longer in the Library".into());
    }
    prepare_into(dir, &request)
}

/// Splits the image into one Piece per Part and adds them all.
#[tauri::command(async)]
fn split_to_library(app: tauri::AppHandle, request: Request) -> Result<Vec<PieceView>, String> {
    let (image, options) = parse(&request)?;
    split_piece(image, &options)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|piece| save(new_piece_dir(&app)?, piece, None))
        .collect()
}

/// A Library piece as the editor reopens it; images and edits in base64.
#[derive(Serialize)]
struct OpenedPiece {
    #[serde(flatten)]
    source: Source,
    image: String,
    edits: Option<String>,
}

#[tauri::command(async)]
fn open_piece(app: tauri::AppHandle, id: String) -> Result<OpenedPiece, String> {
    let dir = piece_dir(&app, &id)?;
    let read = |name: &str| fs::read(dir.join(name)).map_err(|e| e.to_string());
    let (image, source) = match fs::read(dir.join("source.json")) {
        Ok(json) => (
            read("source")?,
            serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        ),
        // A split piece reopens from its own cleaned image.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let piece: Piece =
                serde_json::from_slice(&read("piece.json")?).map_err(|e| e.to_string())?;
            let source = Source {
                mime: "image/png".into(),
                options: piece.reopen_options(),
            };
            (read("piece.png")?, source)
        }
        Err(e) => return Err(e.to_string()),
    };
    Ok(OpenedPiece {
        source,
        image: STANDARD.encode(image),
        edits: fs::read(dir.join("edits.png"))
            .ok()
            .map(|e| STANDARD.encode(e)),
    })
}

#[tauri::command(async)]
fn delete_piece(app: tauri::AppHandle, id: String) -> Result<(), String> {
    fs::remove_dir_all(piece_dir(&app, &id)?).map_err(|e| e.to_string())
}

/// The Piece saved in a Library folder.
fn read_piece(dir: &Path) -> Result<Piece, String> {
    let read = |name: &str| fs::read(dir.join(name)).map_err(|e| e.to_string());
    let mut piece: Piece =
        serde_json::from_slice(&read("piece.json")?).map_err(|e| e.to_string())?;
    piece.png = read("piece.png")?;
    Ok(piece)
}

/// Every Piece in the Library, oldest first. Unreadable folders are skipped.
#[tauri::command(async)]
fn list_library(app: tauri::AppHandle) -> Result<Vec<PieceView>, String> {
    let Ok(entries) = fs::read_dir(library_dir(&app)?) else {
        return Ok(vec![]);
    };
    let mut pieces: Vec<PieceView> = entries
        .flatten()
        .filter_map(|entry| {
            Some(PieceView::new(
                Some(entry.file_name().to_string_lossy().into_owned()),
                read_piece(&entry.path()).ok()?,
            ))
        })
        .collect();
    pieces.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(pieces)
}

/// A Job as the frontend sends it: Library piece ids with their quantities.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobRequest {
    paper: Paper,
    sheets_mm: Vec<[f64; 2]>,
    separation_mm: f64,
    edge_margin_mm: f64,
    time_limit_s: f64,
    items: Vec<ItemRequest>,
}

#[derive(Deserialize)]
struct ItemRequest {
    id: String,
    quantity: Quantity,
}

/// The Layout with each of its pages rendered.
#[derive(Serialize)]
struct NestView {
    pages: Vec<String>,
    #[serde(flatten)]
    layout: Layout,
}

fn render_pages(job: &Job, layout: &Layout, options: &OutputOptions) -> Vec<String> {
    (0..layout.pages())
        .map(|page| render_page(job, layout, page, options))
        .collect()
}

/// The cancellation token of the latest nesting run.
#[derive(Default)]
struct Nesting(Mutex<Arc<AtomicBool>>);

/// What gets printed: the latest run's latest Layout, with its Job, and the options it's rendered
/// with.
#[derive(Default)]
struct Output {
    options: OutputOptions,
    latest: Option<(Arc<Job>, Layout)>,
}

/// Nests on a worker thread, sending every improved Layout to `on_progress` and the iterations so far
/// to `on_iterations` every 250 ms, and returns the best once the time limit is up or
/// `stop_nesting` is called. A new run stops the one in progress. Pages are rendered with `options`
/// until `set_output_options` changes them.
#[tauri::command(async)]
fn nest_job(
    app: tauri::AppHandle,
    nesting: State<Nesting>,
    output: State<Mutex<Output>>,
    job: JobRequest,
    options: OutputOptions,
    on_progress: Channel<NestView>,
    on_iterations: Channel<u64>,
) -> Result<NestView, String> {
    let items = job
        .items
        .into_iter()
        .map(|item| {
            Ok(JobItem {
                piece: read_piece(&piece_dir(&app, &item.id)?)?,
                quantity: item.quantity,
            })
        })
        .collect::<Result<_, String>>()?;
    let job = Arc::new(Job {
        paper: job.paper,
        sheets_mm: job.sheets_mm,
        separation_mm: job.separation_mm,
        edge_margin_mm: job.edge_margin_mm,
        time_limit: Duration::try_from_secs_f64(job.time_limit_s).map_err(|e| e.to_string())?,
        items,
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let previous = std::mem::replace(&mut *nesting.0.lock().unwrap(), cancel.clone());
    previous.store(true, Ordering::Relaxed);
    output.lock().unwrap().options = options;
    // Kept for printing, unless a newer run has started.
    let keep = |layout: Layout| {
        let mut output = output.lock().unwrap();
        let view = NestView {
            pages: render_pages(&job, &layout, &output.options),
            layout: layout.clone(),
        };
        if Arc::ptr_eq(&nesting.0.lock().unwrap(), &cancel) {
            output.latest = Some((job.clone(), layout));
        }
        view
    };
    let iterations = AtomicU64::new(0);
    let layout = thread::scope(|scope| {
        // Ticks until `_done` is dropped, with the nesting.
        let (_done, ticks) = mpsc::channel::<()>();
        let (iterations, on_iterations) = (&iterations, &on_iterations);
        scope.spawn(move || {
            while let Err(RecvTimeoutError::Timeout) =
                ticks.recv_timeout(Duration::from_millis(250))
            {
                _ = on_iterations.send(iterations.load(Ordering::Relaxed));
            }
        });
        // A fixed seed: the same Job gives the same Layouts, as far as the time limit lets it get.
        nest(
            &job,
            0,
            // The frontend may have moved on; nothing to do then.
            |layout| _ = on_progress.send(keep(layout.clone())),
            &cancel,
            iterations,
        )
    })
    .map_err(|e| e.to_string())?;
    Ok(keep(layout))
}

/// Renders the latest Layout's pages again with `options`, which later ones are rendered with too.
#[tauri::command]
fn set_output_options(output: State<Mutex<Output>>, options: OutputOptions) -> Vec<String> {
    let mut output = output.lock().unwrap();
    output.options = options;
    match &output.latest {
        Some((job, layout)) => render_pages(job, layout, &options),
        None => vec![],
    }
}

/// Asks where to save the latest Layout as a PDF and saves it there. False if the jeweller cancels.
#[tauri::command(async)]
fn export_job_pdf(app: tauri::AppHandle, output: State<Mutex<Output>>) -> Result<bool, String> {
    let (options, Some((job, layout))) = ({
        let output = output.lock().unwrap();
        (output.options, output.latest.clone())
    }) else {
        return Err("there is no layout to export".into());
    };
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_file_name("argentajo.pdf")
        .blocking_save_file()
    else {
        return Ok(false);
    };
    let pdf = export_pdf(&job, &layout, &options).map_err(|e| e.to_string())?;
    fs::write(path.into_path().map_err(|e| e.to_string())?, pdf).map_err(|e| e.to_string())?;
    Ok(true)
}

/// "My sheets": the Sheet sizes saved for reuse, width × height in mm, in a JSON file in app data.
fn my_sheets_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("my-sheets.json"))
}

/// None saved yet reads as none.
#[tauri::command(async)]
fn load_my_sheets(app: tauri::AppHandle) -> Result<Vec<[f64; 2]>, String> {
    match fs::read(my_sheets_file(&app)?) {
        Ok(json) => serde_json::from_slice(&json).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

/// Replaces the saved sizes.
#[tauri::command(async)]
fn save_my_sheets(app: tauri::AppHandle, sheets: Vec<[f64; 2]>) -> Result<(), String> {
    let file = my_sheets_file(&app)?;
    fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(file, serde_json::to_vec_pretty(&sheets).unwrap()).map_err(|e| e.to_string())
}

/// Ends the latest nesting run, which then returns the best Layout so far.
#[tauri::command]
fn stop_nesting(nesting: State<Nesting>) {
    nesting.0.lock().unwrap().store(true, Ordering::Relaxed);
}

fn main() {
    // WebKitGTK's DMABUF renderer dies with "Error 71 (Protocol error)" on Wayland with NVIDIA.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: called first thing in main, before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Nesting::default())
        .manage(Mutex::new(Output::default()))
        .invoke_handler(tauri::generate_handler![
            core_version,
            preview_piece,
            add_to_library,
            update_piece,
            split_to_library,
            open_piece,
            delete_piece,
            list_library,
            nest_job,
            stop_nesting,
            set_output_options,
            export_job_pdf,
            load_my_sheets,
            save_my_sheets
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
