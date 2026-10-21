// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argentajo_core::nest::{Job, JobItem, Layout, Paper, nest, render_page};
use argentajo_core::piece::{Piece, PrepareOptions, prepare_piece, split_piece};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tauri::ipc::{InvokeBody, Request};

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

// ponytail: fixed bound on a nesting run; becomes a per-Job setting with stop and live progress (#9).
const NEST_TIME_LIMIT: Duration = Duration::from_secs(60);

/// A Job as the frontend sends it: Library piece ids with their quantities.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobRequest {
    paper: Paper,
    sheet_mm: [f64; 2],
    separation_mm: f64,
    edge_margin_mm: f64,
    items: Vec<ItemRequest>,
}

#[derive(Deserialize)]
struct ItemRequest {
    id: String,
    quantity: u32,
}

/// The Layout with its page rendered.
#[derive(Serialize)]
struct NestView {
    svg: String,
    #[serde(flatten)]
    layout: Layout,
}

#[tauri::command(async)]
fn nest_job(app: tauri::AppHandle, job: JobRequest) -> Result<NestView, String> {
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
    let job = Job {
        paper: job.paper,
        sheet_mm: job.sheet_mm,
        separation_mm: job.separation_mm,
        edge_margin_mm: job.edge_margin_mm,
        items,
    };
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let layout = nest(&job, seed, NEST_TIME_LIMIT).map_err(|e| e.to_string())?;
    Ok(NestView {
        svg: render_page(&job, &layout),
        layout,
    })
}

fn main() {
    // WebKitGTK's DMABUF renderer dies with "Error 71 (Protocol error)" on Wayland with NVIDIA.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: called first thing in main, before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            core_version,
            preview_piece,
            add_to_library,
            update_piece,
            split_to_library,
            open_piece,
            delete_piece,
            list_library,
            nest_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
