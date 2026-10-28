use crate::geom::Mapper;
use crate::stamps::{self, builtin_font, FontFamily};
use crate::types::{color_hex, parse_color, ObjInfo, Rect};
use crate::{EngineError, Result, State};
use pdfium_render::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct NewText {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub size: f32,
    pub color: String,
    #[serde(default)]
    pub family: FontFamily,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Watermark {
    pub text: String,
    pub size: f32,
    pub color: String,
    /// Degrees counter-clockwise as seen on screen.
    pub angle: f32,
    /// "" = all pages
    #[serde(default)]
    pub pages: String,
    #[serde(default)]
    pub behind: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HeaderFooter {
    /// Six slots; `{page}`, `{total}`, `{date}`, `{name}` are substituted.
    #[serde(default)]
    pub top_left: String,
    #[serde(default)]
    pub top_center: String,
    #[serde(default)]
    pub top_right: String,
    #[serde(default)]
    pub bottom_left: String,
    #[serde(default)]
    pub bottom_center: String,
    #[serde(default)]
    pub bottom_right: String,
    pub size: f32,
    pub color: String,
    pub margin: f32,
    #[serde(default)]
    pub start_number: u32,
    #[serde(default)]
    pub pages: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OcrWord {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn kind(o: &PdfPageObject) -> &'static str {
    match o.object_type() {
        PdfPageObjectType::Text => "text",
        PdfPageObjectType::Image => "image",
        PdfPageObjectType::Path => "path",
        PdfPageObjectType::Shading => "shading",
        PdfPageObjectType::XObjectForm => "form",
        _ => "other",
    }
}

impl State {
    pub fn list_objects(&self, id: u32, index: u32, include_paths: bool) -> Result<Vec<ObjInfo>> {
        let page = self.page(id, index)?;
        let map = Mapper::new(&page);
        let mut out = Vec::new();
        for (i, o) in page.objects().iter().enumerate() {
            let k = kind(&o);
            if k == "path" && !include_paths || k == "shading" || k == "other" {
                continue;
            }
            let Ok(q) = o.bounds() else { continue };
            let rect = map.quad_to_view(&q);
            if rect.w <= 0.0 && rect.h <= 0.0 {
                continue;
            }
            let (text, size, font) = match o.as_text_object() {
                Some(t) => (Some(t.text()), t.scaled_font_size().value, t.font().name()),
                None => (None, 0.0, String::new()),
            };
            if matches!(text.as_deref(), Some(s) if s.trim().is_empty()) {
                continue;
            }
            out.push(ObjInfo {
                index: i as u32,
                kind: k.into(),
                rect,
                text,
                font_size: size,
                font_name: font,
                color: o.fill_color().map(|c| color_hex(&c)).unwrap_or_else(|_| "#000000".into()),
            });
            if out.len() >= 20000 {
                break;
            }
        }
        Ok(out)
    }

    /// Replace the text of a text object. When the object's font is an
    /// embedded subset that likely lacks glyphs for the new characters, the
    /// object is re-created in the closest standard font at the same position.
    pub fn set_object_text(&mut self, id: u32, index: u32, obj: u32, text: String) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let (old_text, embedded, font_name, size, serif, bold, italic, mono, color, matrix) = {
            let o = page.objects().get(obj as PdfPageObjectIndex)?;
            let t = o.as_text_object().ok_or_else(|| EngineError::invalid("not a text object"))?;
            let f = t.font();
            (
                t.text(),
                f.is_embedded().unwrap_or(false),
                f.name(),
                t.unscaled_font_size().value,
                f.is_serif(),
                f.weight().map(|w| matches!(w, PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900)).unwrap_or(false) || f.is_bold_reenforced() || f.name().to_lowercase().contains("bold"),
                f.is_italic() || f.name().to_lowercase().contains("italic"),
                f.is_fixed_pitch(),
                o.fill_color().unwrap_or(PdfColor::BLACK),
                o.matrix()?,
            )
        };
        // Embedded fonts are usually subsets: only glyphs already used with
        // this font on the page are known to exist.
        let mut available: std::collections::HashSet<char> = old_text.chars().collect();
        if embedded {
            if let Ok(tp) = page.text() {
                for ch in tp.chars().iter() {
                    if ch.font_name() == font_name {
                        if let Some(c) = ch.unicode_char() {
                            available.insert(c);
                        }
                    }
                }
            }
        }
        let subset = embedded;
        let needs_new_font = subset && text.chars().any(|c| !c.is_whitespace() && !available.contains(&c));
        if !needs_new_font {
            let mut o = page.objects().get(obj as PdfPageObjectIndex)?;
            o.as_text_object_mut().ok_or_else(|| EngineError::invalid("not a text object"))?.set_text(&text)?;
            page.regenerate_content()?;
            return Ok(());
        }
        let family = if mono { FontFamily::Courier } else if serif { FontFamily::Times } else { FontFamily::Helvetica };
        let font = builtin_font(&mut self.doc_mut(id)?.pdf, family, bold, italic);
        let doc = &self.doc(id)?.pdf;
        let mut nt = PdfPageTextObject::new(doc, if text.is_empty() { " " } else { &text }, font, PdfPoints::new(size))?;
        nt.set_fill_color(color)?;
        nt.apply_matrix(matrix)?;
        page.objects_mut().insert_object_at_index(obj as PdfPageObjectIndex, PdfPageObject::from(nt))?;
        // Detached objects are leaked rather than destroyed: destroying a
        // removed object crashes some PDFium builds (see redact.rs).
        std::mem::forget(page.objects_mut().remove_object_at_index((obj + 1) as PdfPageObjectIndex)?);
        page.regenerate_content()?;
        Ok(())
    }

    pub fn move_object(&mut self, id: u32, index: u32, obj: u32, dx: f32, dy: f32) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let a = map.to_page(0.0, 0.0);
        let b = map.to_page(dx, dy);
        let mut o = page.objects().get(obj as PdfPageObjectIndex)?;
        o.translate(PdfPoints::new(b.0 - a.0), PdfPoints::new(b.1 - a.1))?;
        page.regenerate_content()?;
        Ok(())
    }

    /// Scale an object so its view-space bounds become `to`.
    pub fn resize_object(&mut self, id: u32, index: u32, obj: u32, to: Rect) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let mut o = page.objects().get(obj as PdfPageObjectIndex)?;
        let cur = map.quad_to_view(&o.bounds()?);
        if cur.w < 0.01 || cur.h < 0.01 {
            return Ok(());
        }
        let to = to.normalized();
        // Work in page space: anchor at the page-space bottom-left of the box.
        let cur_pr = map.rect_to_page(&cur);
        let to_pr = map.rect_to_page(&to);
        let sx = to_pr.width().value / cur_pr.width().value.max(0.01);
        let sy = to_pr.height().value / cur_pr.height().value.max(0.01);
        o.translate(-cur_pr.left(), -cur_pr.bottom())?;
        o.scale(sx, sy)?;
        o.translate(to_pr.left(), to_pr.bottom())?;
        page.regenerate_content()?;
        Ok(())
    }

    pub fn delete_objects(&mut self, id: u32, index: u32, mut objs: Vec<u32>) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        objs.sort_unstable();
        objs.dedup();
        for o in objs.into_iter().rev() {
            std::mem::forget(page.objects_mut().remove_object_at_index(o as PdfPageObjectIndex)?);
        }
        page.regenerate_content()?;
        Ok(())
    }

    pub fn set_object_color(&mut self, id: u32, index: u32, obj: u32, color: String) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let mut o = page.objects().get(obj as PdfPageObjectIndex)?;
        let c = parse_color(&color);
        o.set_fill_color(c)?;
        if o.object_type() == PdfPageObjectType::Path {
            let _ = o.set_stroke_color(c);
        }
        page.regenerate_content()?;
        Ok(())
    }

    pub fn add_text(&mut self, id: u32, index: u32, t: NewText) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let font = builtin_font(&mut self.doc_mut(id)?.pdf, t.family, t.bold, t.italic);
        let doc = &self.doc(id)?.pdf;
        let c = parse_color(&t.color);
        for (i, line) in t.text.split('\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            let baseline = t.y + t.size * (i as f32 * 1.2 + 0.82);
            let obj = stamps::text_object(doc, &map, line, font, t.size, c, t.x, baseline, 0.0)?;
            page.objects_mut().add_text_object(obj)?;
        }
        page.regenerate_content()?;
        Ok(())
    }

    pub fn add_image(&mut self, id: u32, index: u32, rect: Rect, data: Vec<u8>) -> Result<()> {
        let img = image::load_from_memory(&data)?;
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let doc = &self.doc(id)?.pdf;
        let obj = stamps::image_object(doc, &map, &img, &rect)?;
        page.objects_mut().add_image_object(obj)?;
        page.regenerate_content()?;
        Ok(())
    }

    pub fn replace_image(&mut self, id: u32, index: u32, obj: u32, data: Vec<u8>) -> Result<()> {
        let img = image::load_from_memory(&data)?;
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let mut o = page.objects().get(obj as PdfPageObjectIndex)?;
        o.as_image_object_mut().ok_or_else(|| EngineError::invalid("not an image"))?.set_image(&img)?;
        page.regenerate_content()?;
        Ok(())
    }

    pub fn extract_image(&self, id: u32, index: u32, obj: u32) -> Result<Vec<u8>> {
        let page = self.page(id, index)?;
        let o = page.objects().get(obj as PdfPageObjectIndex)?;
        let doc = &self.doc(id)?.pdf;
        let img = o
            .as_image_object()
            .ok_or_else(|| EngineError::invalid("not an image"))?
            .get_processed_image(doc)
            .or_else(|_| o.as_image_object().unwrap().get_raw_image())?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)?;
        Ok(out.into_inner())
    }

    pub fn watermark(&mut self, id: u32, w: Watermark) -> Result<()> {
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let pages = if w.pages.trim().is_empty() { (0..n).collect() } else { crate::pages::parse_ranges(&w.pages, n)? };
        self.checkpoint(id)?;
        let font = builtin_font(&mut self.doc_mut(id)?.pdf, FontFamily::Helvetica, true, false);
        let c = parse_color(&w.color);
        for p in pages {
            let mut page = self.page(id, p)?;
            let map_page = self.page(id, p)?;
            let map = Mapper::new(&map_page);
            let (pw, ph) = (page.width().value, page.height().value);
            let tw = stamps::approx_width(&w.text, w.size, FontFamily::Helvetica, true);
            let rad = w.angle.to_radians();
            // Start point so the rotated string is centred on the page.
            let (cx, cy) = (pw / 2.0, ph / 2.0);
            let sx = cx - (tw / 2.0) * rad.cos() + (w.size * 0.35) * rad.sin();
            let sy = cy + (tw / 2.0) * rad.sin() + (w.size * 0.35) * rad.cos();
            let doc = &self.doc(id)?.pdf;
            let obj = stamps::text_object(doc, &map, &w.text, font, w.size, c, sx, sy, w.angle)?;
            if w.behind {
                page.objects_mut().insert_object_at_index(0, PdfPageObject::from(obj))?;
            } else {
                page.objects_mut().add_text_object(obj)?;
            }
            page.regenerate_content()?;
        }
        Ok(())
    }

    pub fn header_footer(&mut self, id: u32, hf: HeaderFooter, doc_name: String) -> Result<()> {
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let pages = if hf.pages.trim().is_empty() { (0..n).collect() } else { crate::pages::parse_ranges(&hf.pages, n)? };
        self.checkpoint(id)?;
        let font = builtin_font(&mut self.doc_mut(id)?.pdf, FontFamily::Helvetica, false, false);
        let c = parse_color(&hf.color);
        let start = if hf.start_number == 0 { 1 } else { hf.start_number };
        for (k, p) in pages.iter().enumerate() {
            let mut page = self.page(id, *p)?;
            let map_page = self.page(id, *p)?;
            let map = Mapper::new(&map_page);
            let (pw, ph) = (page.width().value, page.height().value);
            let num = start + k as u32;
            let total = start + pages.len() as u32 - 1;
            let sub = |s: &str| {
                s.replace("{page}", &num.to_string())
                    .replace("{total}", &total.to_string())
                    .replace("{date}", &hf.date)
                    .replace("{name}", &doc_name)
            };
            let slots = [
                (&hf.top_left, 0, true),
                (&hf.top_center, 1, true),
                (&hf.top_right, 2, true),
                (&hf.bottom_left, 0, false),
                (&hf.bottom_center, 1, false),
                (&hf.bottom_right, 2, false),
            ];
            let doc = &self.doc(id)?.pdf;
            for (s, col, top) in slots {
                if s.trim().is_empty() {
                    continue;
                }
                let s = sub(s);
                let tw = stamps::approx_width(&s, hf.size, FontFamily::Helvetica, false);
                let x = match col {
                    0 => hf.margin,
                    1 => (pw - tw) / 2.0,
                    _ => pw - hf.margin - tw,
                };
                let y = if top { hf.margin + hf.size } else { ph - hf.margin };
                let obj = stamps::text_object(doc, &map, &s, font, hf.size, c, x, y, 0.0)?;
                page.objects_mut().add_text_object(obj)?;
            }
            page.regenerate_content()?;
        }
        Ok(())
    }

    /// Add an invisible, selectable/searchable text layer from OCR results.
    pub fn add_ocr_layer(&mut self, id: u32, index: u32, words: Vec<OcrWord>) -> Result<u32> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let font = builtin_font(&mut self.doc_mut(id)?.pdf, FontFamily::Helvetica, false, false);
        let doc = &self.doc(id)?.pdf;
        let mut added = 0;
        for w in words {
            let text = w.text.trim();
            if text.is_empty() || w.w <= 0.0 || w.h <= 0.0 {
                continue;
            }
            let size = w.h * 0.9;
            let natural = stamps::approx_width(text, size, FontFamily::Helvetica, false).max(0.1);
            let sx = (w.w / natural).clamp(0.05, 20.0);
            let mut obj = PdfPageTextObject::new(doc, text, font, PdfPoints::new(size))?;
            obj.set_render_mode(PdfPageTextRenderMode::Invisible)?;
            obj.scale(sx, 1.0)?;
            let rot = map.rotation() as f32;
            if rot > 0.0 {
                obj.rotate_counter_clockwise_degrees(rot)?;
            }
            let (px, py) = map.to_page(w.x, w.y + w.h * 0.8);
            obj.translate(PdfPoints::new(px), PdfPoints::new(py))?;
            page.objects_mut().add_text_object(obj)?;
            added += 1;
        }
        page.regenerate_content()?;
        Ok(added)
    }
}
