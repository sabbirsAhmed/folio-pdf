//! Folio PDF engine.
//!
//! All PDFium access happens on one dedicated thread that owns every open
//! document. Callers submit closures through [`Engine::exec`] and block on the
//! result. This keeps PDFium (which is not thread-safe) isolated, and lets the
//! UI layer stay fully async.
//!
//! Coordinate convention used across the whole public API ("view space"):
//! origin top-left of the page *as displayed* (page /Rotate applied),
//! x to the right, y down, unit = PDF points at 100% zoom.

pub mod annots;
pub mod convert;
pub mod docops;
pub mod edit;
pub mod error;
pub mod forms;
pub mod geom;
pub mod lo;
pub mod pages;
pub mod redact;
pub mod render;
pub mod stamps;
pub mod text;
pub mod types;

use crossbeam_channel::{bounded, unbounded, Sender};
use pdfium_render::prelude::*;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

pub use error::{EngineError, Result};
pub use types::*;

type Job = Box<dyn FnOnce(&mut State) + Send>;

/// Handle to the engine thread. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    tx: Sender<Job>,
}

impl Engine {
    /// Start the engine. `lib_dirs` are searched in order for the PDFium
    /// shared library; the system library path is tried last.
    pub fn start(lib_dirs: Vec<PathBuf>) -> Result<Engine> {
        let (tx, rx) = unbounded::<Job>();
        let (init_tx, init_rx) = bounded::<Result<()>>(1);
        std::thread::Builder::new()
            .name("folio-pdf-engine".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let mut bindings = None;
                let mut last_err = String::new();
                for dir in &lib_dirs {
                    let p = Pdfium::pdfium_platform_library_name_at_path(dir);
                    match Pdfium::bind_to_library(&p) {
                        Ok(b) => {
                            bindings = Some(b);
                            break;
                        }
                        Err(e) => last_err = format!("{}: {e:?}", p.display()),
                    }
                }
                if bindings.is_none() {
                    match Pdfium::bind_to_system_library() {
                        Ok(b) => bindings = Some(b),
                        Err(e) => last_err = format!("{last_err}; system: {e:?}"),
                    }
                }
                let Some(bindings) = bindings else {
                    let _ = init_tx.send(Err(EngineError::new(
                        "pdfium_missing",
                        format!("PDFium library not found ({last_err})"),
                    )));
                    return;
                };
                let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
                let mut state = State::new(pdfium);
                let _ = init_tx.send(Ok(()));
                for job in rx {
                    job(&mut state);
                }
            })
            .map_err(|e| EngineError::new("thread", e.to_string()))?;
        init_rx
            .recv()
            .map_err(|_| EngineError::new("thread", "engine thread died during start"))??;
        Ok(Engine { tx })
    }

    /// Run `f` on the engine thread and wait for its result.
    pub fn exec<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T> + Send + 'static,
    {
        let (rtx, rrx) = bounded::<Result<T>>(1);
        self.tx
            .send(Box::new(move |st: &mut State| {
                let r = catch_unwind(AssertUnwindSafe(|| f(st))).unwrap_or_else(|p| {
                    let msg = p
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown panic".into());
                    Err(EngineError::new("panic", msg))
                });
                let _ = rtx.send(r);
            }))
            .map_err(|_| EngineError::new("thread", "engine thread is not running"))?;
        rrx.recv()
            .map_err(|_| EngineError::new("thread", "engine thread dropped the job"))?
    }
}

/// Password protection to apply when the document is written.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Security {
    pub user_password: String,
    pub owner_password: String,
    pub allow_print: bool,
    pub allow_copy: bool,
    pub allow_modify: bool,
    pub allow_annotate: bool,
    pub allow_forms: bool,
}

pub struct Doc {
    pub pdf: PdfDocument<'static>,
    pub path: Option<PathBuf>,
    pub name: String,
    pub dirty: bool,
    pub security: Option<Security>,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
}

const UNDO_BUDGET_BYTES: usize = 512 * 1024 * 1024;
const UNDO_MAX_STEPS: usize = 50;

pub struct State {
    pub pdfium: &'static Pdfium,
    docs: HashMap<u32, Doc>,
    next_id: u32,
}

impl State {
    fn new(pdfium: &'static Pdfium) -> Self {
        State { pdfium, docs: HashMap::new(), next_id: 1 }
    }

    pub fn doc(&self, id: u32) -> Result<&Doc> {
        self.docs
            .get(&id)
            .ok_or_else(|| EngineError::new("not_found", format!("document {id} is not open")))
    }

    pub fn doc_mut(&mut self, id: u32) -> Result<&mut Doc> {
        self.docs
            .get_mut(&id)
            .ok_or_else(|| EngineError::new("not_found", format!("document {id} is not open")))
    }

    pub fn load_bytes(&self, bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDocument<'static>> {
        self.pdfium.load_pdf_from_byte_vec(bytes, password).map_err(EngineError::from)
    }

    fn insert(&mut self, pdf: PdfDocument<'static>, path: Option<PathBuf>, name: String, security: Option<Security>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(
            id,
            Doc { pdf, path, name, dirty: false, security, undo: vec![], redo: vec![] },
        );
        id
    }

    /// Open a file. Returns `password_required` / `password_incorrect` errors
    /// for encrypted files so the UI can prompt.
    pub fn open(&mut self, path: &Path, password: Option<String>) -> Result<DocInfo> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "document.pdf".into());
        let id = self.open_bytes_named(bytes, password, Some(path.to_path_buf()), name)?;
        self.info(id)
    }

    pub fn open_bytes_named(
        &mut self,
        bytes: Vec<u8>,
        password: Option<String>,
        path: Option<PathBuf>,
        name: String,
    ) -> Result<u32> {
        match self.pdfium.load_pdf_from_byte_vec(bytes.clone(), None) {
            Ok(pdf) => Ok(self.insert(pdf, path, name, None)),
            Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
                let Some(pw) = password else {
                    return Err(EngineError::new("password_required", "This document is password protected."));
                };
                // Verify the password with PDFium first.
                let encrypted = self.pdfium.load_pdf_from_byte_vec(bytes.clone(), Some(&pw)).map_err(|e| match e {
                    PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                        EngineError::new("password_incorrect", "The password is incorrect.")
                    }
                    other => other.into(),
                })?;
                // Work on a decrypted copy so every later operation (lopdf-based
                // ones included) sees plain objects. Protection is re-applied on save.
                let security = Some(Security {
                    user_password: pw.clone(),
                    owner_password: pw.clone(),
                    allow_print: true,
                    allow_copy: true,
                    allow_modify: true,
                    allow_annotate: true,
                    allow_forms: true,
                });
                let plain = remove_security(self.pdfium, &bytes, &pw).or_else(|_| lo::decrypt_to_plain(&bytes, &pw));
                let pdf = match plain {
                    Ok(plain) => self.load_bytes(plain, None).unwrap_or(encrypted),
                    Err(_) => encrypted,
                };
                Ok(self.insert(pdf, path, name, security))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn create_blank(&mut self, width: f32, height: f32, pages: u32) -> Result<DocInfo> {
        let mut pdf = self.pdfium.create_new_pdf()?;
        let size = PdfPagePaperSize::Custom(PdfPoints::new(width), PdfPoints::new(height));
        for _ in 0..pages.max(1) {
            let n = pdf.pages().len();
            pdf.pages_mut().create_page_at_index(size, n)?;
        }
        let bytes = pdf.save_to_bytes()?;
        let id = self.open_bytes_named(bytes, None, None, "Untitled.pdf".into())?;
        self.doc_mut(id)?.dirty = true;
        self.info(id)
    }

    pub fn close(&mut self, id: u32) -> Result<()> {
        self.docs.remove(&id);
        Ok(())
    }

    /// Snapshot the current state onto the undo stack. Call before any mutation.
    pub fn checkpoint(&mut self, id: u32) -> Result<()> {
        let doc = self.doc_mut(id)?;
        let snap = doc.pdf.save_to_bytes()?;
        doc.undo.push(snap);
        doc.redo.clear();
        let mut total: usize = doc.undo.iter().map(|b| b.len()).sum();
        while doc.undo.len() > 1 && (doc.undo.len() > UNDO_MAX_STEPS || total > UNDO_BUDGET_BYTES) {
            total -= doc.undo.remove(0).len();
        }
        doc.dirty = true;
        Ok(())
    }

    pub fn undo(&mut self, id: u32) -> Result<DocInfo> {
        let pdfium = self.pdfium;
        let doc = self.doc_mut(id)?;
        let Some(prev) = doc.undo.pop() else { return Err(EngineError::new("nothing", "Nothing to undo")) };
        let cur = doc.pdf.save_to_bytes()?;
        doc.redo.push(cur);
        doc.pdf = pdfium.load_pdf_from_byte_vec(prev, None)?;
        doc.dirty = true;
        self.info(id)
    }

    pub fn redo(&mut self, id: u32) -> Result<DocInfo> {
        let pdfium = self.pdfium;
        let doc = self.doc_mut(id)?;
        let Some(next) = doc.redo.pop() else { return Err(EngineError::new("nothing", "Nothing to redo")) };
        let cur = doc.pdf.save_to_bytes()?;
        doc.undo.push(cur);
        doc.pdf = pdfium.load_pdf_from_byte_vec(next, None)?;
        doc.dirty = true;
        self.info(id)
    }

    /// Replace the document contents with new bytes (used by lopdf-based ops).
    pub fn replace_bytes(&mut self, id: u32, bytes: Vec<u8>) -> Result<()> {
        let pdf = self.load_bytes(bytes, None)?;
        let doc = self.doc_mut(id)?;
        doc.pdf = pdf;
        doc.dirty = true;
        Ok(())
    }

    /// Final bytes as they would be written to disk (security applied).
    pub fn output_bytes(&self, id: u32) -> Result<Vec<u8>> {
        let doc = self.doc(id)?;
        let bytes = doc.pdf.save_to_bytes()?;
        match &doc.security {
            Some(sec) => lo::encrypt(&bytes, sec),
            None => Ok(bytes),
        }
    }

    pub fn save(&mut self, id: u32, path: Option<PathBuf>) -> Result<DocInfo> {
        let target = match path {
            Some(p) => p,
            None => self
                .doc(id)?
                .path
                .clone()
                .ok_or_else(|| EngineError::new("no_path", "Choose where to save this document."))?,
        };
        let bytes = self.output_bytes(id)?;
        write_atomic(&target, &bytes)?;
        let doc = self.doc_mut(id)?;
        doc.name = target
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| doc.name.clone());
        doc.path = Some(target);
        doc.dirty = false;
        self.info(id)
    }

    /// Save a copy without changing the open document's path or dirty flag.
    pub fn save_copy(&self, id: u32, path: &Path) -> Result<()> {
        let bytes = self.output_bytes(id)?;
        write_atomic(path, &bytes)
    }

    pub fn set_security(&mut self, id: u32, security: Option<Security>) -> Result<DocInfo> {
        let doc = self.doc_mut(id)?;
        doc.security = security;
        doc.dirty = true;
        self.info(id)
    }

    pub fn info(&self, id: u32) -> Result<DocInfo> {
        let doc = self.doc(id)?;
        let mut pages = Vec::new();
        for (i, p) in doc.pdf.pages().iter().enumerate() {
            let rot = match p.rotation().unwrap_or(PdfPageRenderRotation::None) {
                PdfPageRenderRotation::None => 0,
                PdfPageRenderRotation::Degrees90 => 90,
                PdfPageRenderRotation::Degrees180 => 180,
                PdfPageRenderRotation::Degrees270 => 270,
            };
            pages.push(PageInfo {
                index: i as u32,
                width: p.width().value,
                height: p.height().value,
                rotation: rot,
                label: p.label().map(|s| s.to_string()),
            });
        }
        let md = doc.pdf.metadata();
        let get = |t: PdfDocumentMetadataTagType| md.get(t).map(|v| v.value().to_string()).unwrap_or_default();
        let has_forms = doc.pdf.form().is_some();
        Ok(DocInfo {
            id,
            name: doc.name.clone(),
            path: doc.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            page_count: pages.len() as u32,
            pages,
            title: get(PdfDocumentMetadataTagType::Title),
            author: get(PdfDocumentMetadataTagType::Author),
            subject: get(PdfDocumentMetadataTagType::Subject),
            keywords: get(PdfDocumentMetadataTagType::Keywords),
            creator: get(PdfDocumentMetadataTagType::Creator),
            producer: get(PdfDocumentMetadataTagType::Producer),
            created: get(PdfDocumentMetadataTagType::CreationDate),
            modified: get(PdfDocumentMetadataTagType::ModificationDate),
            version: format!("{:?}", doc.pdf.version()),
            has_forms,
            protected: doc.security.is_some(),
            dirty: doc.dirty,
            can_undo: !doc.undo.is_empty(),
            can_redo: !doc.redo.is_empty(),
        })
    }

    pub fn outline(&self, id: u32) -> Result<Vec<OutlineItem>> {
        let doc = self.doc(id)?;
        fn walk(b: &PdfBookmark, depth: usize) -> OutlineItem {
            let page = b
                .destination()
                .and_then(|d| d.page_index().ok())
                .or_else(|| {
                    b.action().and_then(|a| match a {
                        PdfAction::LocalDestination(l) => l.destination().ok().and_then(|d| d.page_index().ok()),
                        _ => None,
                    })
                })
                .map(|p| p as u32);
            let mut children = vec![];
            if depth < 16 {
                let mut c = b.first_child();
                while let Some(cb) = c {
                    children.push(walk(&cb, depth + 1));
                    if children.len() > 2000 {
                        break;
                    }
                    c = cb.next_sibling();
                }
            }
            OutlineItem { title: b.title().unwrap_or_default(), page, children }
        }
        let mut out = Vec::new();
        let mut cur = doc.pdf.bookmarks().root();
        while let Some(b) = cur {
            out.push(walk(&b, 0));
            if out.len() > 5000 {
                break;
            }
            cur = b.next_sibling();
        }
        Ok(out)
    }

    pub fn page(&self, id: u32, index: u32) -> Result<PdfPage<'static>> {
        let doc = self.doc(id)?;
        doc.pdf
            .pages()
            .get(index as PdfPageIndex)
            .map_err(|_| EngineError::new("invalid", format!("page {} does not exist", index + 1)))
    }
}

/// Decrypt a password-protected file into an unencrypted byte copy using
/// PDFium's own save path with FPDF_REMOVE_SECURITY.
fn remove_security(pdfium: &Pdfium, bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    use std::os::raw::{c_int, c_ulong, c_void};
    #[repr(C)]
    struct Writer {
        base: FPDF_FILEWRITE,
        buf: Vec<u8>,
    }
    unsafe extern "C" fn write_block(this: *mut FPDF_FILEWRITE, data: *const c_void, size: c_ulong) -> c_int {
        // SAFETY: `this` is the first field of a live, #[repr(C)] Writer.
        let w = unsafe { &mut *(this as *mut Writer) };
        if size > 0 && !data.is_null() {
            w.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) });
        }
        1
    }
    const FPDF_REMOVE_SECURITY: u32 = 3;
    let b = pdfium.bindings();
    let handle = b.FPDF_LoadMemDocument64(bytes, Some(password));
    if handle.is_null() {
        return Err(EngineError::new("password_incorrect", "The password is incorrect."));
    }
    let mut w = Writer { base: FPDF_FILEWRITE { version: 1, WriteBlock: Some(write_block) }, buf: Vec::new() };
    let ok = b.FPDF_SaveAsCopy(handle, &mut w as *mut Writer as *mut FPDF_FILEWRITE, FPDF_REMOVE_SECURITY as _);
    b.FPDF_CloseDocument(handle);
    if b.is_true(ok) && !w.buf.is_empty() {
        Ok(w.buf)
    } else {
        Err(EngineError::new("decrypt", "could not remove security"))
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.folio-tmp",
        path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Cross-device or locked target (Windows): fall back to direct write.
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(path, bytes).map_err(|_| EngineError::from(e))?;
    }
    Ok(())
}
