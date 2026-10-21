// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argentajo_core::piece::{Piece, PrepareError, PrepareOptions, prepare_piece, split_piece};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
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
fn prepare<T>(
    request: &Request,
    run: fn(&[u8], &PrepareOptions) -> Result<T, PrepareError>,
) -> Result<T, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the image bytes as the request body".into());
    };
    let header = |name| request.headers().get(name).map(|v| v.as_bytes());
    let mut options: PrepareOptions =
        serde_json::from_slice(header("options").ok_or("missing options header")?)
            .map_err(|e| e.to_string())?;
    let edits_length = match header("edits-length") {
        Some(length) => std::str::from_utf8(length)
            .ok()
            .and_then(|length| length.parse().ok())
            .filter(|&length| length <= bytes.len())
            .ok_or("invalid edits-length header")?,
        None => 0,
    };
    let (image, edits) = bytes.split_at(bytes.len() - edits_length);
    options.edits = (edits_length > 0).then(|| edits.to_vec());
    run(image, &options).map_err(|e| e.to_string())
}

// `async`: sync commands run on the main thread and would freeze the window while preparing.
#[tauri::command(async)]
fn preview_piece(request: Request) -> Result<PieceView, String> {
    Ok(PieceView::new(None, prepare(&request, prepare_piece)?))
}

fn library_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("library"))
}

/// Library layout: one folder per Piece holding `piece.png` and `piece.json`.
fn save(app: &tauri::AppHandle, piece: Piece) -> Result<PieceView, String> {
    let library = library_dir(app)?;
    fs::create_dir_all(&library).map_err(|e| e.to_string())?;
    // A split saves several in a row: on a coarse clock the next id may still be taken.
    let (id, dir) = loop {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let dir = library.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => break (id, dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    fs::write(dir.join("piece.png"), &piece.png).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("piece.json"),
        serde_json::to_vec_pretty(&piece).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(PieceView::new(Some(id), piece))
}

#[tauri::command(async)]
fn add_to_library(app: tauri::AppHandle, request: Request) -> Result<PieceView, String> {
    save(&app, prepare(&request, prepare_piece)?)
}

/// Splits the image into one Piece per Part and adds them all.
#[tauri::command(async)]
fn split_to_library(app: tauri::AppHandle, request: Request) -> Result<Vec<PieceView>, String> {
    prepare(&request, split_piece)?
        .into_iter()
        .map(|piece| save(&app, piece))
        .collect()
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
            let dir = entry.path();
            let mut piece: Piece =
                serde_json::from_slice(&fs::read(dir.join("piece.json")).ok()?).ok()?;
            piece.png = fs::read(dir.join("piece.png")).ok()?;
            Some(PieceView::new(
                Some(entry.file_name().to_string_lossy().into_owned()),
                piece,
            ))
        })
        .collect();
    pieces.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(pieces)
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
            split_to_library,
            list_library
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
