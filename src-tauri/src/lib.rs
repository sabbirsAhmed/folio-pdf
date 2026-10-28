//! Tauri shell: exposes the PDF engine to the web UI as commands.

use folio_engine::annots::{b64_decode, NewAnnot};
use folio_engine::edit::{HeaderFooter, NewText, OcrWord, Watermark};
use folio_engine::forms::{FieldValue, NewField};
use folio_engine::lo::{CompressLevel, CompressReport, MetadataUpdate, SanitizeOpts};
use folio_engine::redact::{RedactArea, RedactReport};
use folio_engine::render::RenderOpts;
use folio_engine::{
    AnnotInfo, DocInfo, Engine, EngineError, FieldInfo, ObjInfo, OutlineItem, Rect, Result, SearchHit, Security, TextChar,
};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{ipc::Response, Emitter, Manager};

struct App {
    engine: Engine,
    pending: Mutex<Vec<String>>,
}

async fn exec<T, F>(app: &App, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut folio_engine::State) -> Result<T> + Send + 'static,
{
    let e = app.engine.clone();
    tauri::async_runtime::spawn_blocking(move || e.exec(f))
        .await
        .map_err(|e| EngineError::new("join", e.to_string()))?
}

type S<'a> = tauri::State<'a, App>;

fn bytes_arg(data: &str) -> Result<Vec<u8>> {
    b64_decode(data).map_err(EngineError::invalid)
}

// ------------------------------------------------------------- documents

#[tauri::command]
async fn open_file(s: S<'_>, path: String, password: Option<String>) -> Result<DocInfo> {
    exec(&s, move |st| st.open(&PathBuf::from(path), password)).await
}

#[tauri::command]
async fn create_blank(s: S<'_>, width: f32, height: f32, pages: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.create_blank(width, height, pages)).await
}

#[tauri::command]
async fn close_doc(s: S<'_>, id: u32) -> Result<()> {
    exec(&s, move |st| st.close(id)).await
}

#[tauri::command]
async fn doc_info(s: S<'_>, id: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.info(id)).await
}

#[tauri::command]
async fn outline(s: S<'_>, id: u32) -> Result<Vec<OutlineItem>> {
    exec(&s, move |st| st.outline(id)).await
}

#[tauri::command]
async fn set_outline(s: S<'_>, id: u32, items: Vec<OutlineItem>) -> Result<DocInfo> {
    exec(&s, move |st| st.set_outline(id, items)).await
}

#[tauri::command]
async fn save(s: S<'_>, id: u32, path: Option<String>) -> Result<DocInfo> {
    exec(&s, move |st| st.save(id, path.map(PathBuf::from))).await
}

#[tauri::command]
async fn save_copy(s: S<'_>, id: u32, path: String) -> Result<()> {
    exec(&s, move |st| st.save_copy(id, &PathBuf::from(path))).await
}

#[tauri::command]
async fn undo(s: S<'_>, id: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.undo(id)).await
}

#[tauri::command]
async fn redo(s: S<'_>, id: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.redo(id)).await
}

#[tauri::command]
async fn set_metadata(s: S<'_>, id: u32, meta: MetadataUpdate) -> Result<DocInfo> {
    exec(&s, move |st| st.set_metadata(id, meta)).await
}

#[tauri::command]
async fn set_security(s: S<'_>, id: u32, security: Option<Security>) -> Result<DocInfo> {
    exec(&s, move |st| st.set_security(id, security)).await
}

#[tauri::command]
async fn sanitize(s: S<'_>, id: u32, opts: SanitizeOpts) -> Result<DocInfo> {
    exec(&s, move |st| st.sanitize(id, opts)).await
}

#[tauri::command]
async fn compress(s: S<'_>, id: u32, level: CompressLevel) -> Result<CompressReport> {
    exec(&s, move |st| st.compress(id, level)).await
}

// ------------------------------------------------------------- rendering & text

#[tauri::command]
async fn render_page(s: S<'_>, id: u32, page: u32, scale: f32, annotations: Option<bool>, forms: Option<bool>) -> Result<Response> {
    let opts = RenderOpts { scale, annotations: annotations.unwrap_or(true), forms: forms.unwrap_or(true) };
    let bytes = exec(&s, move |st| st.render_bmp(id, page, opts)).await?;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn render_png(s: S<'_>, id: u32, page: u32, scale: f32) -> Result<Response> {
    let bytes = exec(&s, move |st| st.render_png(id, page, scale)).await?;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn page_chars(s: S<'_>, id: u32, page: u32) -> Result<Vec<TextChar>> {
    exec(&s, move |st| st.page_chars(id, page)).await
}

#[tauri::command]
async fn page_has_text(s: S<'_>, id: u32, page: u32) -> Result<bool> {
    exec(&s, move |st| st.page_has_text(id, page)).await
}

#[tauri::command]
async fn search(s: S<'_>, id: u32, query: String, match_case: bool, whole_word: bool) -> Result<Vec<SearchHit>> {
    exec(&s, move |st| st.search(id, &query, match_case, whole_word, 5000)).await
}

// ------------------------------------------------------------- annotations

#[tauri::command]
async fn list_annots(s: S<'_>, id: u32, page: u32) -> Result<Vec<AnnotInfo>> {
    exec(&s, move |st| st.list_annots(id, page)).await
}

#[tauri::command]
async fn add_annot(s: S<'_>, id: u32, page: u32, spec: NewAnnot, author: String) -> Result<()> {
    exec(&s, move |st| st.add_annot(id, page, spec, &author)).await
}

#[tauri::command]
async fn delete_annot(s: S<'_>, id: u32, page: u32, index: u32) -> Result<()> {
    exec(&s, move |st| st.delete_annot(id, page, index)).await
}

#[tauri::command]
async fn update_annot(s: S<'_>, id: u32, page: u32, index: u32, contents: Option<String>, color: Option<String>, rect: Option<Rect>) -> Result<()> {
    exec(&s, move |st| st.update_annot(id, page, index, contents, color, rect)).await
}

#[tauri::command]
async fn delete_all_annots(s: S<'_>, id: u32, kinds: Vec<String>) -> Result<u32> {
    exec(&s, move |st| st.delete_all_annots(id, kinds)).await
}

// ------------------------------------------------------------- forms

#[tauri::command]
async fn list_fields(s: S<'_>, id: u32, page: u32) -> Result<Vec<FieldInfo>> {
    exec(&s, move |st| st.list_fields(id, page)).await
}

#[tauri::command]
async fn set_field(s: S<'_>, id: u32, page: u32, index: u32, value: FieldValue) -> Result<()> {
    exec(&s, move |st| st.set_field(id, page, index, value)).await
}

#[tauri::command]
async fn add_field(s: S<'_>, id: u32, spec: NewField) -> Result<()> {
    exec(&s, move |st| st.add_field(id, spec)).await
}

#[tauri::command]
async fn delete_field(s: S<'_>, id: u32, page: u32, index: u32) -> Result<()> {
    exec(&s, move |st| st.delete_field(id, page, index)).await
}

#[tauri::command]
async fn flatten(s: S<'_>, id: u32) -> Result<DocInfo> {
    exec(&s, move |st| {
        st.flatten(id)?;
        st.info(id)
    })
    .await
}

#[tauri::command]
async fn reset_form(s: S<'_>, id: u32) -> Result<()> {
    exec(&s, move |st| st.reset_form(id)).await
}

#[tauri::command]
async fn export_form_data(s: S<'_>, id: u32, path: String) -> Result<()> {
    let v = exec(&s, move |st| st.export_form_data(id)).await?;
    std::fs::write(path, serde_json::to_vec_pretty(&v).unwrap_or_default()).map_err(EngineError::from)
}

#[tauri::command]
async fn import_form_data(s: S<'_>, id: u32, path: String) -> Result<u32> {
    let raw = std::fs::read(path)?;
    let v: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| EngineError::invalid(e.to_string()))?;
    exec(&s, move |st| st.import_form_data(id, v)).await
}

// ------------------------------------------------------------- pages

#[tauri::command]
async fn rotate_pages(s: S<'_>, id: u32, pages: Vec<u32>, delta: i32) -> Result<DocInfo> {
    exec(&s, move |st| st.rotate_pages(id, pages, delta)).await
}

#[tauri::command]
async fn delete_pages(s: S<'_>, id: u32, pages: Vec<u32>) -> Result<DocInfo> {
    exec(&s, move |st| st.delete_pages(id, pages)).await
}

#[tauri::command]
async fn insert_blank(s: S<'_>, id: u32, at: u32, width: f32, height: f32) -> Result<DocInfo> {
    exec(&s, move |st| st.insert_blank(id, at, width, height)).await
}

#[tauri::command]
async fn insert_from_file(s: S<'_>, id: u32, at: u32, path: String, pages: String, password: Option<String>) -> Result<DocInfo> {
    exec(&s, move |st| st.insert_from_file(id, at, &PathBuf::from(path), &pages, password)).await
}

#[tauri::command]
async fn duplicate_pages(s: S<'_>, id: u32, pages: Vec<u32>) -> Result<DocInfo> {
    exec(&s, move |st| st.duplicate_pages(id, pages)).await
}

#[tauri::command]
async fn move_pages(s: S<'_>, id: u32, pages: Vec<u32>, to: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.move_pages(id, pages, to)).await
}

#[tauri::command]
async fn reorder_pages(s: S<'_>, id: u32, order: Vec<u32>) -> Result<DocInfo> {
    exec(&s, move |st| st.reorder_pages(id, order)).await
}

#[tauri::command]
async fn reverse_pages(s: S<'_>, id: u32) -> Result<DocInfo> {
    exec(&s, move |st| st.reverse_pages(id)).await
}

#[tauri::command]
async fn extract_pages(s: S<'_>, id: u32, pages: Vec<u32>, path: String, delete_after: bool) -> Result<DocInfo> {
    exec(&s, move |st| {
        st.extract_pages(id, pages, &PathBuf::from(path), delete_after)?;
        st.info(id)
    })
    .await
}

#[tauri::command]
async fn split(s: S<'_>, id: u32, every: Option<u32>, ranges: Option<String>, max_mb: Option<f64>, out_dir: String, stem: String) -> Result<Vec<String>> {
    let max_bytes = max_mb.map(|m| (m * 1024.0 * 1024.0) as u64);
    exec(&s, move |st| st.split(id, every, ranges, max_bytes, &PathBuf::from(out_dir), &stem)).await
}

#[tauri::command]
async fn combine(s: S<'_>, files: Vec<String>) -> Result<DocInfo> {
    exec(&s, move |st| st.combine(files.into_iter().map(|f| (PathBuf::from(f), None)).collect())).await
}

#[tauri::command]
async fn crop_pages(s: S<'_>, id: u32, pages: Vec<u32>, rect: Rect) -> Result<DocInfo> {
    exec(&s, move |st| st.crop_pages(id, pages, rect)).await
}

// ------------------------------------------------------------- content editing

#[tauri::command]
async fn list_objects(s: S<'_>, id: u32, page: u32, include_paths: bool) -> Result<Vec<ObjInfo>> {
    exec(&s, move |st| st.list_objects(id, page, include_paths)).await
}

#[tauri::command]
async fn set_object_text(s: S<'_>, id: u32, page: u32, obj: u32, text: String) -> Result<()> {
    exec(&s, move |st| st.set_object_text(id, page, obj, text)).await
}

#[tauri::command]
async fn move_object(s: S<'_>, id: u32, page: u32, obj: u32, dx: f32, dy: f32) -> Result<()> {
    exec(&s, move |st| st.move_object(id, page, obj, dx, dy)).await
}

#[tauri::command]
async fn resize_object(s: S<'_>, id: u32, page: u32, obj: u32, rect: Rect) -> Result<()> {
    exec(&s, move |st| st.resize_object(id, page, obj, rect)).await
}

#[tauri::command]
async fn delete_objects(s: S<'_>, id: u32, page: u32, objs: Vec<u32>) -> Result<()> {
    exec(&s, move |st| st.delete_objects(id, page, objs)).await
}

#[tauri::command]
async fn set_object_color(s: S<'_>, id: u32, page: u32, obj: u32, color: String) -> Result<()> {
    exec(&s, move |st| st.set_object_color(id, page, obj, color)).await
}

#[tauri::command]
async fn add_text(s: S<'_>, id: u32, page: u32, text: NewText) -> Result<()> {
    exec(&s, move |st| st.add_text(id, page, text)).await
}

#[tauri::command]
async fn add_image(s: S<'_>, id: u32, page: u32, rect: Rect, path: Option<String>, data: Option<String>) -> Result<()> {
    let bytes = match (path, data) {
        (Some(p), _) => std::fs::read(p)?,
        (None, Some(d)) => bytes_arg(&d)?,
        _ => return Err(EngineError::invalid("no image supplied")),
    };
    exec(&s, move |st| st.add_image(id, page, rect, bytes)).await
}

#[tauri::command]
async fn replace_image(s: S<'_>, id: u32, page: u32, obj: u32, path: String) -> Result<()> {
    let bytes = std::fs::read(path)?;
    exec(&s, move |st| st.replace_image(id, page, obj, bytes)).await
}

#[tauri::command]
async fn extract_image(s: S<'_>, id: u32, page: u32, obj: u32, path: String) -> Result<()> {
    let png = exec(&s, move |st| st.extract_image(id, page, obj)).await?;
    std::fs::write(path, png).map_err(EngineError::from)
}

#[tauri::command]
async fn watermark(s: S<'_>, id: u32, spec: Watermark) -> Result<()> {
    exec(&s, move |st| st.watermark(id, spec)).await
}

#[tauri::command]
async fn header_footer(s: S<'_>, id: u32, spec: HeaderFooter, name: String) -> Result<()> {
    exec(&s, move |st| st.header_footer(id, spec, name)).await
}

#[tauri::command]
async fn add_ocr_layer(s: S<'_>, id: u32, page: u32, words: Vec<OcrWord>) -> Result<u32> {
    exec(&s, move |st| st.add_ocr_layer(id, page, words)).await
}

#[tauri::command]
async fn apply_redactions(s: S<'_>, id: u32, areas: Vec<RedactArea>, fill: String) -> Result<RedactReport> {
    exec(&s, move |st| st.apply_redactions(id, areas, fill)).await
}

// ------------------------------------------------------------- export

#[tauri::command]
async fn export_docx(s: S<'_>, id: u32, path: String, images: bool) -> Result<()> {
    exec(&s, move |st| st.export_docx(id, &PathBuf::from(path), images)).await
}

#[tauri::command]
async fn export_xlsx(s: S<'_>, id: u32, path: String) -> Result<()> {
    exec(&s, move |st| st.export_xlsx(id, &PathBuf::from(path))).await
}

#[tauri::command]
async fn export_text(s: S<'_>, id: u32, path: String) -> Result<()> {
    exec(&s, move |st| st.export_text(id, &PathBuf::from(path))).await
}

#[tauri::command]
async fn export_images(s: S<'_>, id: u32, pages: Vec<u32>, dpi: f32, format: String, out_dir: String, stem: String) -> Result<Vec<String>> {
    exec(&s, move |st| st.export_images(id, pages, dpi, &format, &PathBuf::from(out_dir), &stem)).await
}

// ------------------------------------------------------------- shell

#[tauri::command]
fn take_pending_files(s: S<'_>) -> Vec<String> {
    std::mem::take(&mut *s.pending.lock().unwrap())
}

#[tauri::command]
fn print_window(window: tauri::WebviewWindow) -> std::result::Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_in_folder(app: tauri::AppHandle, path: String) -> std::result::Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_externally(app: tauri::AppHandle, path: String) -> std::result::Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// Read a small image file (signature upload) as a data URL.
#[tauri::command]
fn read_image_data_url(path: String) -> std::result::Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("Image is larger than 20 MB".into());
    }
    let mime = match path.rsplit('.').next().map(|e| e.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    };
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 32);
    out.push_str("data:");
    out.push_str(mime);
    out.push_str(";base64,");
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    Ok(out)
}

#[tauri::command]
fn user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "User".into())
}

fn pdf_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| {
            let l = a.to_lowercase();
            l.ends_with(".pdf") || l.starts_with("file://")
        })
        .map(|a| {
            if let Some(rest) = a.strip_prefix("file://") {
                percent_decode(rest)
            } else {
                a
            }
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn pdfium_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Ok(r) = app.path().resource_dir() {
        dirs.push(r.join("pdfium"));
        dirs.push(r.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf());
            dirs.push(d.join("pdfium"));
            dirs.push(d.join("../Frameworks"));
            dirs.push(d.join("../Resources/pdfium"));
        }
    }
    // Development checkout.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/pdfium-bundle");
    dirs.push(dev);
    dirs
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let files = pdf_args(argv);
            if !files.is_empty() {
                let _ = app.emit("open-files", files);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let engine = Engine::start(pdfium_dirs(app.handle())).map_err(|e| {
                let msg = format!(
                    "Folio could not load its PDF engine.\n\n{}\n\nReinstall the application, or place the PDFium library next to the executable.",
                    e.message
                );
                use tauri_plugin_dialog::DialogExt;
                app.dialog().message(msg.clone()).title("Folio PDF").blocking_show();
                Box::<dyn std::error::Error>::from(msg)
            })?;
            app.manage(App { engine, pending: Mutex::new(pdf_args(std::env::args())) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_file, create_blank, close_doc, doc_info, outline, set_outline, save, save_copy, undo, redo,
            set_metadata, set_security, sanitize, compress,
            render_page, render_png, page_chars, page_has_text, search,
            list_annots, add_annot, delete_annot, update_annot, delete_all_annots,
            list_fields, set_field, add_field, delete_field, flatten, reset_form, export_form_data, import_form_data,
            rotate_pages, delete_pages, insert_blank, insert_from_file, duplicate_pages, move_pages, reorder_pages,
            reverse_pages, extract_pages, split, combine, crop_pages,
            list_objects, set_object_text, move_object, resize_object, delete_objects, set_object_color, add_text,
            add_image, replace_image, extract_image, watermark, header_footer, add_ocr_layer, apply_redactions,
            export_docx, export_xlsx, export_text, export_images,
            take_pending_files, print_window, reveal_in_folder, open_externally, user_name, read_image_data_url
        ])
        .build(tauri::generate_context!())
        .expect("error while building Folio PDF");

    app.run(|handle, event| {
        // macOS delivers "Open With" / double-clicked files as an event.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        if let tauri::RunEvent::Opened { urls } = &event {
            let files: Vec<String> = urls
                .iter()
                .filter_map(|u| u.to_file_path().ok())
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            if let Some(state) = handle.try_state::<App>() {
                state.pending.lock().unwrap().extend(files.clone());
            }
            let _ = handle.emit("open-files", files);
        }
        let _ = (&handle, &event);
    });
}
