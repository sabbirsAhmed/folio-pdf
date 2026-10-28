use crate::geom::Mapper;
use crate::stamps::{self, builtin_font, FontFamily};
use crate::types::{color_hex, parse_color, AnnotInfo, Rect};
use crate::{EngineError, Result, State};
use pdfium_render::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewAnnot {
    /// Text markup over runs of text. One rect per line.
    Markup { style: MarkupStyle, rects: Vec<Rect>, color: String, #[serde(default)] note: String },
    /// Freehand pen strokes.
    Ink { strokes: Vec<Vec<[f32; 2]>>, color: String, width: f32 },
    Shape { shape: ShapeKind, rect: Rect, color: String, width: f32, #[serde(default)] fill: Option<String> },
    Line { from: [f32; 2], to: [f32; 2], color: String, width: f32, #[serde(default)] arrow: bool },
    /// Sticky note.
    Note { x: f32, y: f32, text: String, color: String },
    /// Typed text box.
    TextBox {
        rect: Rect,
        text: String,
        size: f32,
        color: String,
        #[serde(default)]
        family: FontFamily,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        border: Option<String>,
    },
    /// Rubber stamp with a label (APPROVED, DRAFT, ...).
    Stamp { rect: Rect, label: String, color: String },
    /// Image stamp: signatures, initials, logos. `png` is base64-free raw bytes.
    Image { rect: Rect, #[serde(with = "serde_bytes_compat")] data: Vec<u8> },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkupStyle {
    Highlight,
    Underline,
    Strikeout,
    Squiggly,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rect,
    Ellipse,
}

/// Accepts either a JSON array of numbers or a base64 string.
pub mod serde_bytes_compat {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum B {
            Arr(Vec<u8>),
            Str(String),
        }
        match B::deserialize(d)? {
            B::Arr(v) => Ok(v),
            B::Str(s) => super::b64_decode(&s).map_err(serde::de::Error::custom),
        }
    }
}

pub fn b64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    let s = s.split(',').next_back().unwrap_or(s); // strip data: URL prefix
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("invalid base64 byte {c}")),
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn kind_name(t: PdfPageAnnotationType) -> &'static str {
    use PdfPageAnnotationType as T;
    match t {
        T::Text => "note",
        T::FreeText => "free_text",
        T::Highlight => "highlight",
        T::Underline => "underline",
        T::Strikeout => "strikeout",
        T::Squiggly => "squiggly",
        T::Ink => "ink",
        T::Square => "square",
        T::Circle => "circle",
        T::Stamp => "stamp",
        T::Line => "line",
        T::Polygon => "polygon",
        T::Polyline => "polyline",
        T::Link => "link",
        T::Widget => "widget",
        T::Popup => "popup",
        T::Redacted => "redact",
        T::FileAttachment => "attachment",
        _ => "other",
    }
}

impl State {
    pub fn list_annots(&self, id: u32, index: u32) -> Result<Vec<AnnotInfo>> {
        let page = self.page(id, index)?;
        let map = Mapper::new(&page);
        let mut out = Vec::new();
        for (i, a) in page.annotations().iter().enumerate() {
            let t = a.annotation_type();
            if matches!(t, PdfPageAnnotationType::Popup | PdfPageAnnotationType::Widget | PdfPageAnnotationType::XfaWidget) {
                continue;
            }
            let rect = a.bounds().map(|b| map.rect_to_view(&b)).unwrap_or_default();
            // NB: PdfPageAnnotation::stroke_color() is unsafe to call on
            // annotations that have an appearance stream (pdfium-render casts
            // the annotation handle to a page-object handle). Read colour from
            // the appearance objects instead.
            let color = annot_color(&a);
            out.push(AnnotInfo {
                index: i as u32,
                kind: kind_name(t).to_string(),
                rect,
                color,
                contents: a.contents().unwrap_or_default(),
                author: a.creator().unwrap_or_default(),
                modified: a.modification_date().unwrap_or_default(),
            });
        }
        Ok(out)
    }

    pub fn add_annot(&mut self, id: u32, index: u32, spec: NewAnnot, author: &str) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let map_page = self.page(id, index)?;
        let map = Mapper::new(&map_page);
        let doc = &mut self.doc_mut(id)?.pdf;

        match spec {
            NewAnnot::Markup { style, rects, color, note } => {
                if rects.is_empty() {
                    return Err(EngineError::invalid("no text selected"));
                }
                let c = parse_color(&color);
                let bounds = rects.iter().skip(1).fold(rects[0], |a, b| a.union(b));
                let quads: Vec<PdfQuadPoints> = rects.iter().map(|r| map.rect_to_quad(r)).collect();
                let annots = page.annotations_mut();
                macro_rules! fill {
                    ($a:expr) => {{
                        let mut a = $a;
                        a.set_bounds(map.rect_to_page(&bounds))?;
                        a.set_stroke_color(c)?;
                        for q in &quads {
                            a.attachment_points_mut().create_attachment_point_at_end(*q)?;
                        }
                        if !note.is_empty() {
                            a.set_contents(&note)?;
                        }
                        a.set_creator(author)?;
                    }};
                }
                match style {
                    MarkupStyle::Highlight => fill!(annots.create_highlight_annotation()?),
                    MarkupStyle::Underline => fill!(annots.create_underline_annotation()?),
                    MarkupStyle::Strikeout => fill!(annots.create_strikeout_annotation()?),
                    MarkupStyle::Squiggly => fill!(annots.create_squiggly_annotation()?),
                }
            }
            NewAnnot::Ink { strokes, color, width } => {
                let c = parse_color(&color);
                let pts: Vec<Vec<(f32, f32)>> =
                    strokes.iter().filter(|s| !s.is_empty()).map(|s| s.iter().map(|p| (p[0], p[1])).collect()).collect();
                if pts.is_empty() {
                    return Err(EngineError::invalid("empty drawing"));
                }
                let bounds = bounds_of(&pts, width);
                let paths: Vec<_> = pts
                    .iter()
                    .map(|s| stamps::polyline_path(doc, &map, s, c, width, false, None))
                    .collect::<Result<_>>()?;
                let mut a = page.annotations_mut().create_ink_annotation()?;
                a.set_bounds(map.rect_to_page(&bounds))?;
                a.set_stroke_color(c)?;
                a.set_creator(author)?;
                for p in paths {
                    a.objects_mut().add_path_object(p)?;
                }
            }
            NewAnnot::Shape { shape, rect, color, width, fill } => {
                let c = parse_color(&color);
                let f = fill.as_deref().map(parse_color);
                let path = match shape {
                    ShapeKind::Rect => stamps::rect_path(doc, &map, &rect, Some((c, width)), f)?,
                    ShapeKind::Ellipse => {
                        stamps::polyline_path(doc, &map, &stamps::ellipse_points(&rect, 72), c, width, true, f)?
                    }
                };
                let bounds = Rect { x: rect.normalized().x - width, y: rect.normalized().y - width, w: rect.w.abs() + 2.0 * width, h: rect.h.abs() + 2.0 * width };
                let mut a = page.annotations_mut().create_ink_annotation()?;
                a.set_bounds(map.rect_to_page(&bounds))?;
                a.set_stroke_color(c)?;
                a.set_creator(author)?;
                a.set_contents(match shape {
                    ShapeKind::Rect => "Rectangle",
                    ShapeKind::Ellipse => "Ellipse",
                })?;
                a.objects_mut().add_path_object(path)?;
            }
            NewAnnot::Line { from, to, color, width, arrow } => {
                let c = parse_color(&color);
                let mut strokes = vec![vec![(from[0], from[1]), (to[0], to[1])]];
                if arrow {
                    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
                    let len = (dx * dx + dy * dy).sqrt().max(0.001);
                    let (ux, uy) = (dx / len, dy / len);
                    let head = (width * 4.0).max(8.0);
                    let back = (to[0] - ux * head, to[1] - uy * head);
                    let (px, py) = (-uy * head * 0.5, ux * head * 0.5);
                    strokes.push(vec![(back.0 + px, back.1 + py), (to[0], to[1]), (back.0 - px, back.1 - py)]);
                }
                let bounds = bounds_of(&strokes, width * 4.0);
                let paths: Vec<_> = strokes
                    .iter()
                    .map(|s| stamps::polyline_path(doc, &map, s, c, width, false, None))
                    .collect::<Result<_>>()?;
                let mut a = page.annotations_mut().create_ink_annotation()?;
                a.set_bounds(map.rect_to_page(&bounds))?;
                a.set_stroke_color(c)?;
                a.set_creator(author)?;
                a.set_contents(if arrow { "Arrow" } else { "Line" })?;
                for p in paths {
                    a.objects_mut().add_path_object(p)?;
                }
            }
            NewAnnot::Note { x, y, text, color } => {
                let mut a = page.annotations_mut().create_text_annotation(&text)?;
                a.set_bounds(map.rect_to_page(&Rect { x, y, w: 20.0, h: 20.0 }))?;
                a.set_fill_color(parse_color(&color))?;
                a.set_stroke_color(parse_color(&color))?;
                a.set_creator(author)?;
            }
            NewAnnot::TextBox { rect, text, size, color, family, bold, border } => {
                let c = parse_color(&color);
                let font = builtin_font(doc, family, bold, false);
                let r = rect.normalized();
                let mut objs = Vec::new();
                if let Some(b) = border.as_deref() {
                    objs.push(PdfPageObject::from(stamps::rect_path(doc, &map, &r, Some((parse_color(b), 1.0)), None)?));
                }
                let lines = wrap_lines(&text, r.w - 4.0, size, family, bold);
                for (i, line) in lines.iter().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let by = r.y + 2.0 + size * (i as f32 + 1.0) * 1.18 - size * 0.18;
                    objs.push(PdfPageObject::from(stamps::text_object(doc, &map, line, font, size, c, r.x + 2.0, by, 0.0)?));
                }
                let used_h = (lines.len() as f32 * size * 1.18 + 4.0).max(r.h);
                let bounds = Rect { x: r.x, y: r.y, w: r.w, h: used_h };
                let mut a = page.annotations_mut().create_stamp_annotation()?;
                a.set_bounds(map.rect_to_page(&bounds))?;
                a.set_contents(&text)?;
                a.set_creator(author)?;
                for o in objs {
                    a.objects_mut().add_object(o)?;
                }
            }
            NewAnnot::Stamp { rect, label, color } => {
                let c = parse_color(&color);
                let r = rect.normalized();
                let font = builtin_font(doc, FontFamily::Helvetica, true, false);
                let border = stamps::rect_path(doc, &map, &r, Some((c, 2.5)), None)?;
                let inner = Rect { x: r.x + 3.5, y: r.y + 3.5, w: r.w - 7.0, h: r.h - 7.0 };
                let inner = stamps::rect_path(doc, &map, &inner, Some((c, 0.8)), None)?;
                let mut size = r.h * 0.5;
                let w = stamps::approx_width(&label, size, FontFamily::Helvetica, true);
                if w > r.w * 0.86 {
                    size *= r.w * 0.86 / w;
                }
                let tw = stamps::approx_width(&label, size, FontFamily::Helvetica, true);
                let tx = r.x + (r.w - tw) / 2.0;
                let ty = r.y + r.h / 2.0 + size * 0.35;
                let t = stamps::text_object(doc, &map, &label, font, size, c, tx, ty, 0.0)?;
                let mut a = page.annotations_mut().create_stamp_annotation()?;
                a.set_bounds(map.rect_to_page(&r))?;
                a.set_contents(&label)?;
                a.set_creator(author)?;
                a.objects_mut().add_path_object(border)?;
                a.objects_mut().add_path_object(inner)?;
                a.objects_mut().add_text_object(t)?;
            }
            NewAnnot::Image { rect, data } => {
                let img = image::load_from_memory(&data)?;
                let r = rect.normalized();
                let obj = stamps::image_object(doc, &map, &img, &r)?;
                let mut a = page.annotations_mut().create_stamp_annotation()?;
                a.set_bounds(map.rect_to_page(&r))?;
                a.set_creator(author)?;
                a.set_contents("Signature")?;
                a.objects_mut().add_image_object(obj)?;
            }
        }
        // Without the Print flag PDFium (and Acrobat) skip the annotation when
        // printing and when flattening for print.
        if let Ok(mut last) = page.annotations().last() {
            last.set_is_printed(true)?;
        }
        Ok(())
    }

    pub fn delete_annot(&mut self, id: u32, index: u32, annot: u32) -> Result<()> {
        self.checkpoint(id)?;
        let mut page = self.page(id, index)?;
        let a = page.annotations().get(annot as PdfPageAnnotationIndex)?;
        page.annotations_mut().delete_annotation(a)?;
        Ok(())
    }

    pub fn update_annot(&mut self, id: u32, index: u32, annot: u32, contents: Option<String>, color: Option<String>, rect: Option<Rect>) -> Result<()> {
        self.checkpoint(id)?;
        let page = self.page(id, index)?;
        let map_page_guard = self.page(id, index)?;
        let map = Mapper::new(&map_page_guard);
        let mut a = page.annotations().get(annot as PdfPageAnnotationIndex)?;
        if let Some(c) = contents {
            a.set_contents(&c)?;
        }
        if let Some(r) = rect {
            // Moving is only meaningful for annotations whose appearance is
            // positioned by its rect (notes). Others keep their artwork.
            a.set_bounds(map.rect_to_page(&r))?;
        }
        if let Some(c) = color {
            let pc = parse_color(&c);
            if a.objects().len() > 0 {
                // Ink/stamp artwork: recolour the appearance objects directly.
                for i in 0..a.objects().len() {
                    let mut o = a.objects().get(i)?;
                    if o.object_type() == PdfPageObjectType::Text {
                        let _ = o.set_fill_color(pc);
                    } else {
                        let _ = o.set_stroke_color(pc);
                    }
                }
                a.set_stroke_color(pc).ok();
            } else {
                drop(a);
                drop(page);
                drop(map_page_guard);
                let bytes = self.doc(id)?.pdf.save_to_bytes()?;
                let out = crate::lo::set_annot_color(&bytes, index, annot, &c)?;
                self.replace_bytes(id, out)?;
            }
        }
        Ok(())
    }

    /// Delete all annotations of the given kinds (empty = all comments).
    pub fn delete_all_annots(&mut self, id: u32, kinds: Vec<String>) -> Result<u32> {
        self.checkpoint(id)?;
        let n = self.doc(id)?.pdf.pages().len();
        let mut removed = 0;
        for pi in 0..n {
            let mut page = self.page(id, pi as u32)?;
            let count = page.annotations().len();
            for ai in (0..count).rev() {
                let a = page.annotations().get(ai)?;
                let t = a.annotation_type();
                if matches!(t, PdfPageAnnotationType::Widget | PdfPageAnnotationType::Link | PdfPageAnnotationType::XfaWidget) {
                    continue;
                }
                if kinds.is_empty() || kinds.iter().any(|k| k == kind_name(t)) {
                    page.annotations_mut().delete_annotation(a)?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

fn annot_color(a: &PdfPageAnnotation) -> String {
    let objs = a.objects();
    for i in 0..objs.len() {
        if let Ok(o) = objs.get(i) {
            let c = match o.object_type() {
                PdfPageObjectType::Text => o.fill_color(),
                _ => o.stroke_color().or_else(|_| o.fill_color()),
            };
            if let Ok(c) = c {
                return color_hex(&c);
            }
        }
    }
    String::new()
}

fn bounds_of(strokes: &[Vec<(f32, f32)>], pad: f32) -> Rect {
    let mut r: Option<Rect> = None;
    for s in strokes {
        for p in s {
            let pr = Rect { x: p.0 - pad, y: p.1 - pad, w: pad * 2.0, h: pad * 2.0 };
            r = Some(match r {
                Some(a) => a.union(&pr),
                None => pr,
            });
        }
    }
    r.unwrap_or_default()
}

/// Greedy word wrap using approximate metrics of the standard fonts.
pub fn wrap_lines(text: &str, width: f32, size: f32, family: FontFamily, bold: bool) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && stamps::approx_width(&candidate, size, family, bold) > width {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}
