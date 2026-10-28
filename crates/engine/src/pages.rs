use crate::geom::Mapper;
use crate::lo;
use crate::types::{DocInfo, Rect};
use crate::{EngineError, Result, State};
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};

fn rot_from(deg: i32) -> PdfPageRenderRotation {
    match deg.rem_euclid(360) {
        90 => PdfPageRenderRotation::Degrees90,
        180 => PdfPageRenderRotation::Degrees180,
        270 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    }
}

fn rot_deg(r: PdfPageRenderRotation) -> i32 {
    match r {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 90,
        PdfPageRenderRotation::Degrees180 => 180,
        PdfPageRenderRotation::Degrees270 => 270,
    }
}

/// "1-3, 5, 8-" (1-based, inclusive) -> zero-based indices.
pub fn parse_ranges(spec: &str, count: u32) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let a: u32 = if a.is_empty() { 1 } else { a.parse().map_err(|_| EngineError::invalid(format!("bad page range \"{part}\"")))? };
        let b: u32 = if b.is_empty() { count } else { b.parse().map_err(|_| EngineError::invalid(format!("bad page range \"{part}\"")))? };
        if a == 0 || b == 0 || a > count || b > count {
            return Err(EngineError::invalid(format!("page range \"{part}\" is outside 1-{count}")));
        }
        if a <= b {
            out.extend((a - 1)..b);
        } else {
            out.extend(((b - 1)..a).rev());
        }
    }
    Ok(out)
}

fn to_pdfium_ranges(pages: &[u32]) -> String {
    pages.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(",")
}

impl State {
    fn page_count(&self, id: u32) -> Result<u32> {
        Ok(self.doc(id)?.pdf.pages().len() as u32)
    }

    pub fn rotate_pages(&mut self, id: u32, pages: Vec<u32>, delta: i32) -> Result<DocInfo> {
        self.checkpoint(id)?;
        for p in pages {
            let mut page = self.page(id, p)?;
            let cur = rot_deg(page.rotation().unwrap_or(PdfPageRenderRotation::None));
            page.set_rotation(rot_from(cur + delta));
        }
        self.info(id)
    }

    pub fn delete_pages(&mut self, id: u32, mut pages: Vec<u32>) -> Result<DocInfo> {
        let n = self.page_count(id)?;
        pages.sort_unstable();
        pages.dedup();
        if pages.len() as u32 >= n {
            return Err(EngineError::invalid("a document must keep at least one page"));
        }
        self.checkpoint(id)?;
        for p in pages.into_iter().rev() {
            self.page(id, p)?.delete()?;
        }
        self.info(id)
    }

    pub fn insert_blank(&mut self, id: u32, at: u32, width: f32, height: f32) -> Result<DocInfo> {
        self.checkpoint(id)?;
        let size = PdfPagePaperSize::Custom(PdfPoints::new(width), PdfPoints::new(height));
        self.doc_mut(id)?.pdf.pages_mut().create_page_at_index(size, at as PdfPageIndex)?;
        self.info(id)
    }

    /// Insert pages of another PDF at `at`. `pages` is a range spec for the
    /// source ("" = all).
    pub fn insert_from_file(&mut self, id: u32, at: u32, path: &Path, pages: &str, password: Option<String>) -> Result<DocInfo> {
        let bytes = std::fs::read(path)?;
        let src = self.pdfium.load_pdf_from_byte_vec(bytes, password.as_deref()).map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                EngineError::new("password_required", format!("{} is password protected", path.display()))
            }
            o => o.into(),
        })?;
        let count = src.pages().len() as u32;
        let list = if pages.trim().is_empty() { (0..count).collect() } else { parse_ranges(pages, count)? };
        self.checkpoint(id)?;
        let doc = self.doc_mut(id)?;
        doc.pdf.pages_mut().copy_pages_from_document(&src, &to_pdfium_ranges(&list), at as PdfPageIndex)?;
        self.info(id)
    }

    pub fn duplicate_pages(&mut self, id: u32, pages: Vec<u32>) -> Result<DocInfo> {
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let copy = self.load_bytes(bytes, None)?;
        self.checkpoint(id)?;
        let mut sorted = pages.clone();
        sorted.sort_unstable();
        for p in sorted.into_iter().rev() {
            self.doc_mut(id)?.pdf.pages_mut().copy_page_from_document(&copy, p as PdfPageIndex, (p + 1) as PdfPageIndex)?;
        }
        self.info(id)
    }

    pub fn reorder_pages(&mut self, id: u32, order: Vec<u32>) -> Result<DocInfo> {
        self.checkpoint(id)?;
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::reorder_pages(&bytes, &order)?;
        self.replace_bytes(id, out)?;
        self.info(id)
    }

    /// Move a set of pages so they start at `to` (index in the final order).
    pub fn move_pages(&mut self, id: u32, mut pages: Vec<u32>, to: u32) -> Result<DocInfo> {
        let n = self.page_count(id)?;
        pages.sort_unstable();
        pages.dedup();
        let rest: Vec<u32> = (0..n).filter(|p| !pages.contains(p)).collect();
        let to = (to as usize).min(rest.len());
        let mut order = rest[..to].to_vec();
        order.extend(&pages);
        order.extend(&rest[to..]);
        self.reorder_pages(id, order)
    }

    pub fn reverse_pages(&mut self, id: u32) -> Result<DocInfo> {
        let n = self.page_count(id)?;
        self.reorder_pages(id, (0..n).rev().collect())
    }

    /// Write selected pages to a new PDF file.
    pub fn extract_pages(&mut self, id: u32, pages: Vec<u32>, out: &Path, delete_after: bool) -> Result<()> {
        let mut new = self.pdfium.create_new_pdf()?;
        {
            let src = &self.doc(id)?.pdf;
            new.pages_mut().copy_pages_from_document(src, &to_pdfium_ranges(&pages), 0)?;
        }
        crate::write_atomic(out, &new.save_to_bytes()?)?;
        if delete_after {
            self.delete_pages(id, pages)?;
        }
        Ok(())
    }

    /// Split into several files. Either every `every` pages, or at explicit
    /// range specs ("1-3;4-9;10-").
    pub fn split(&self, id: u32, every: Option<u32>, ranges: Option<String>, max_bytes: Option<u64>, out_dir: &Path, stem: &str) -> Result<Vec<String>> {
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let groups: Vec<Vec<u32>> = if let Some(r) = ranges {
            r.split(';').map(|s| parse_ranges(s, n)).collect::<Result<_>>()?
        } else if let Some(limit) = max_bytes {
            self.groups_by_size(id, limit)?
        } else {
            let e = every.unwrap_or(1).max(1);
            (0..n).collect::<Vec<_>>().chunks(e as usize).map(|c| c.to_vec()).collect()
        };
        let mut written = Vec::new();
        let src = &self.doc(id)?.pdf;
        for (i, g) in groups.iter().enumerate() {
            if g.is_empty() {
                continue;
            }
            let mut new = self.pdfium.create_new_pdf()?;
            new.pages_mut().copy_pages_from_document(src, &to_pdfium_ranges(g), 0)?;
            let path: PathBuf = out_dir.join(format!("{stem} - part {}.pdf", i + 1));
            crate::write_atomic(&path, &new.save_to_bytes()?)?;
            written.push(path.to_string_lossy().to_string());
        }
        Ok(written)
    }

    fn groups_by_size(&self, id: u32, limit: u64) -> Result<Vec<Vec<u32>>> {
        let src = &self.doc(id)?.pdf;
        let n = src.pages().len() as u32;
        let mut groups = vec![];
        let mut cur: Vec<u32> = vec![];
        for p in 0..n {
            let mut trial = cur.clone();
            trial.push(p);
            let mut t = self.pdfium.create_new_pdf()?;
            t.pages_mut().copy_pages_from_document(src, &to_pdfium_ranges(&trial), 0)?;
            let size = t.save_to_bytes()?.len() as u64;
            if size > limit && !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
                cur.push(p);
            } else {
                cur = trial;
            }
        }
        if !cur.is_empty() {
            groups.push(cur);
        }
        Ok(groups)
    }

    /// Combine files into a new, unsaved document and open it.
    pub fn combine(&mut self, files: Vec<(PathBuf, Option<String>)>) -> Result<DocInfo> {
        if files.is_empty() {
            return Err(EngineError::invalid("no files to combine"));
        }
        let mut new = self.pdfium.create_new_pdf()?;
        for (path, pw) in &files {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"].contains(&ext.as_str()) {
                let img = image::open(path)?;
                append_image_page(&mut new, &img)?;
                continue;
            }
            let bytes = std::fs::read(path)?;
            let src = self.pdfium.load_pdf_from_byte_vec(bytes, pw.as_deref()).map_err(|e| match e {
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                    EngineError::new("password_required", format!("{} is password protected", path.display()))
                }
                o => o.into(),
            })?;
            new.pages_mut().append(&src)?;
        }
        let bytes = new.save_to_bytes()?;
        let id = self.open_bytes_named(bytes, None, None, "Combined.pdf".into())?;
        self.doc_mut(id)?.dirty = true;
        self.info(id)
    }

    /// Create a PDF from images, one page per image.
    pub fn from_images(&mut self, files: Vec<PathBuf>) -> Result<DocInfo> {
        self.combine(files.into_iter().map(|p| (p, None)).collect())
    }

    pub fn crop_pages(&mut self, id: u32, pages: Vec<u32>, rect: Rect) -> Result<DocInfo> {
        self.checkpoint(id)?;
        for p in pages {
            let mut page = self.page(id, p)?;
            let map_page = self.page(id, p)?;
            let map = Mapper::new(&map_page);
            let r = map.rect_to_page(&rect);
            page.boundaries_mut().set_crop(r)?;
        }
        self.info(id)
    }
}

fn append_image_page(doc: &mut PdfDocument<'static>, img: &image::DynamicImage) -> Result<()> {
    // 96 dpi mapping keeps phone photos to a sensible physical size, capped to A3.
    let mut w = img.width() as f32 * 0.75;
    let mut h = img.height() as f32 * 0.75;
    let max = 1190.0;
    if w.max(h) > max {
        let s = max / w.max(h);
        w *= s;
        h *= s;
    }
    let n = doc.pages().len();
    let mut page = doc
        .pages_mut()
        .create_page_at_index(PdfPagePaperSize::Custom(PdfPoints::new(w), PdfPoints::new(h)), n)?;
    let obj = PdfPageImageObject::new_with_size(doc, img, PdfPoints::new(w), PdfPoints::new(h))?;
    page.objects_mut().add_image_object(obj)?;
    Ok(())
}
