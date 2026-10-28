//! Export to Word, Excel, images and plain text.
//!
//! Text reconstruction works from PDFium's character stream: characters are
//! grouped into lines by baseline, lines into paragraphs by spacing and
//! indentation, and (for spreadsheets) line fragments into cells by gaps,
//! with columns found by clustering cell start positions across the page.

use crate::types::TextChar;
use crate::{EngineError, Result, State};
use docx_rs::{BreakType, Docx, Paragraph, Pic, Run};
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct Glyph {
    c: String,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    size: f32,
    bold: bool,
    italic: bool,
}

#[derive(Clone, Debug, Default)]
struct Line {
    glyphs: Vec<Glyph>,
    y0: f32,
    y1: f32,
}

impl Line {
    fn x0(&self) -> f32 {
        self.glyphs.first().map(|g| g.x0).unwrap_or(0.0)
    }
    fn size(&self) -> f32 {
        let mut s: Vec<f32> = self.glyphs.iter().map(|g| g.size).collect();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s.get(s.len() / 2).copied().unwrap_or(10.0)
    }
    fn text(&self) -> String {
        let mut out = String::new();
        let mut prev: Option<&Glyph> = None;
        for g in &self.glyphs {
            if let Some(p) = prev {
                let gap = g.x0 - p.x1;
                if gap > g.size.max(1.0) * 0.22 && !out.ends_with(' ') && g.c != " " {
                    out.push(' ');
                }
            }
            out.push_str(&g.c);
            prev = Some(g);
        }
        out
    }
    /// Split into cells at large horizontal gaps.
    fn cells(&self) -> Vec<(f32, String)> {
        let mut cells: Vec<(f32, String)> = vec![];
        let mut cur = String::new();
        let mut start = 0.0;
        let mut prev: Option<&Glyph> = None;
        for g in &self.glyphs {
            if let Some(p) = prev {
                let gap = g.x0 - p.x1;
                if gap > g.size.max(1.0) * 1.4 {
                    cells.push((start, cur.trim().to_string()));
                    cur.clear();
                    start = g.x0;
                } else if gap > g.size.max(1.0) * 0.22 && !cur.ends_with(' ') && g.c != " " {
                    cur.push(' ');
                }
            } else {
                start = g.x0;
            }
            cur.push_str(&g.c);
            prev = Some(g);
        }
        if !cur.trim().is_empty() {
            cells.push((start, cur.trim().to_string()));
        }
        cells.into_iter().filter(|(_, s)| !s.is_empty()).collect()
    }
}

fn build_lines(chars: &[TextChar], styles: &[(bool, bool)]) -> Vec<Line> {
    let mut lines: Vec<Line> = vec![];
    let mut cur = Line::default();
    for (i, ch) in chars.iter().enumerate() {
        if ch.g && (ch.c == "\r" || ch.c == "\n" || ch.c == "\r\n") {
            if !cur.glyphs.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if ch.c == "\r" || ch.c == "\n" {
            continue;
        }
        if ch.g && ch.c == " " {
            continue; // spaces are re-inferred from geometry
        }
        let (bold, italic) = styles.get(i).copied().unwrap_or((false, false));
        let g = Glyph { c: ch.c.clone(), x0: ch.r.x, x1: ch.r.x + ch.r.w, y0: ch.r.y, y1: ch.r.y + ch.r.h, size: ch.size, bold, italic };
        if g.c.trim().is_empty() && cur.glyphs.is_empty() {
            continue;
        }
        if let Some(last) = cur.glyphs.last() {
            let mid = (g.y0 + g.y1) / 2.0;
            let lh = (cur.y1 - cur.y0).max(1.0);
            let new_line = mid < cur.y0 - lh * 0.1 || mid > cur.y1 + lh * 0.1 || g.x0 < last.x0 - last.size * 4.0;
            if new_line {
                lines.push(std::mem::take(&mut cur));
            }
        }
        if cur.glyphs.is_empty() {
            cur.y0 = g.y0;
            cur.y1 = g.y1;
        } else {
            cur.y0 = cur.y0.min(g.y0);
            cur.y1 = cur.y1.max(g.y1);
        }
        cur.glyphs.push(g);
    }
    if !cur.glyphs.is_empty() {
        lines.push(cur);
    }
    for l in &mut lines {
        l.glyphs.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
    }
    lines.sort_by(|a, b| {
        let ay = (a.y0 + a.y1) / 2.0;
        let by = (b.y0 + b.y1) / 2.0;
        if (ay - by).abs() < (a.y1 - a.y0).min(b.y1 - b.y0) * 0.5 {
            a.x0().partial_cmp(&b.x0()).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    lines
}

/// Group lines into paragraphs.
fn paragraphs(lines: &[Line]) -> Vec<Vec<&Line>> {
    let mut out: Vec<Vec<&Line>> = vec![];
    for l in lines {
        let start_new = match out.last().and_then(|p| p.last()) {
            None => true,
            Some(prev) => {
                let gap = l.y0 - prev.y1;
                let size = prev.size().max(1.0);
                gap > size * 0.9 || (l.size() - prev.size()).abs() > 1.5 || (l.x0() - prev.x0()).abs() > size * 3.0
            }
        };
        if start_new {
            out.push(vec![l]);
        } else {
            out.last_mut().unwrap().push(l);
        }
    }
    out
}

impl State {
    fn char_styles(&self, id: u32, index: u32) -> Result<(Vec<TextChar>, Vec<(bool, bool)>)> {
        let chars = self.page_chars(id, index)?;
        let page = self.page(id, index)?;
        let text = page.text()?;
        let mut styles = Vec::with_capacity(chars.len());
        for ch in text.chars().iter() {
            if ch.unicode_string().map(|s| s.is_empty()).unwrap_or(true) {
                continue;
            }
            let bold = ch
                .font_weight()
                .map(|w| matches!(w, PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900))
                .unwrap_or(false)
                || ch.font_name().to_lowercase().contains("bold");
            let italic = ch.font_is_italic() || ch.font_name().to_lowercase().contains("italic");
            styles.push((bold, italic));
        }
        Ok((chars, styles))
    }

    pub fn export_text(&self, id: u32, out: &Path) -> Result<()> {
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let mut s = String::new();
        for p in 0..n {
            let (chars, styles) = self.char_styles(id, p)?;
            for para in paragraphs(&build_lines(&chars, &styles)) {
                let t: Vec<String> = para.iter().map(|l| l.text()).collect();
                s.push_str(&t.join(" "));
                s.push_str("\n\n");
            }
            s.push('\u{c}');
        }
        crate::write_atomic(out, s.as_bytes())
    }

    pub fn export_docx(&self, id: u32, out: &Path, include_images: bool) -> Result<()> {
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let mut docx = Docx::new();
        if let Some(first) = self.doc(id)?.pdf.pages().iter().next() {
            // twips = points * 20
            let (w, h) = (first.width().value, first.height().value);
            docx = docx.page_size((w * 20.0) as u32, (h * 20.0) as u32).page_margin(
                docx_rs::PageMargin::new().top(1080).bottom(1080).left(1080).right(1080),
            );
        }
        for p in 0..n {
            let (chars, styles) = self.char_styles(id, p)?;
            let lines = build_lines(&chars, &styles);
            let mut items: Vec<(f32, Paragraph)> = vec![];
            for para in paragraphs(&lines) {
                let mut pg = Paragraph::new();
                let first = para[0];
                let size = first.size();
                // Runs grouped by (bold, italic).
                let mut buf = String::new();
                let mut style = (false, false);
                let flush = |pg: Paragraph, buf: &mut String, style: (bool, bool)| -> Paragraph {
                    if buf.is_empty() {
                        return pg;
                    }
                    let mut r = Run::new().add_text(std::mem::take(buf)).size((size * 2.0).round() as usize);
                    if style.0 {
                        r = r.bold();
                    }
                    if style.1 {
                        r = r.italic();
                    }
                    pg.add_run(r)
                };
                for (li, line) in para.iter().enumerate() {
                    if li > 0 && !buf.ends_with(' ') {
                        buf.push(' ');
                    }
                    let mut prev: Option<&Glyph> = None;
                    for g in &line.glyphs {
                        let st = (g.bold, g.italic);
                        if st != style && !g.c.trim().is_empty() {
                            pg = flush(pg, &mut buf, style);
                            style = st;
                        }
                        if let Some(pv) = prev {
                            if g.x0 - pv.x1 > g.size.max(1.0) * 0.22 && !buf.ends_with(' ') && g.c != " " {
                                buf.push(' ');
                            }
                        }
                        buf.push_str(&g.c);
                        prev = Some(g);
                    }
                }
                pg = flush(pg, &mut buf, style);
                if size >= 15.0 {
                    pg = pg.style("Heading1");
                }
                items.push((first.y0, pg));
            }
            if include_images {
                let page = self.page(id, p)?;
                let map = crate::geom::Mapper::new(&page);
                let doc = &self.doc(id)?.pdf;
                for o in page.objects().iter() {
                    let Some(img) = o.as_image_object() else { continue };
                    let Ok(q) = o.bounds() else { continue };
                    let r = map.quad_to_view(&q);
                    if r.w < 24.0 || r.h < 24.0 {
                        continue;
                    }
                    let Ok(dimg) = img.get_processed_image(doc) else { continue };
                    let mut png = std::io::Cursor::new(Vec::new());
                    if dimg.write_to(&mut png, image::ImageFormat::Png).is_err() {
                        continue;
                    }
                    let emu = |pt: f32| (pt * 12700.0) as u32;
                    let pic = Pic::new(&png.into_inner()).size(emu(r.w.min(500.0)), emu(r.h * (r.w.min(500.0) / r.w)));
                    items.push((r.y, Paragraph::new().add_run(Run::new().add_image(pic))));
                }
            }
            items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let count = items.len();
            for (i, (_, mut pg)) in items.into_iter().enumerate() {
                if i + 1 == count && p + 1 < n {
                    pg = pg.add_run(Run::new().add_break(BreakType::Page));
                }
                docx = docx.add_paragraph(pg);
            }
            if count == 0 && p + 1 < n {
                docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
            }
        }
        let file = std::fs::File::create(out)?;
        docx.build().pack(file).map_err(|e| EngineError::new("docx", e.to_string()))?;
        Ok(())
    }

    pub fn export_xlsx(&self, id: u32, out: &Path) -> Result<()> {
        use rust_xlsxwriter::{Format, Workbook};
        let n = self.doc(id)?.pdf.pages().len() as u32;
        let mut wb = Workbook::new();
        let bold = Format::new().set_bold();
        for p in 0..n {
            let (chars, styles) = self.char_styles(id, p)?;
            let lines = build_lines(&chars, &styles);
            // Merge fragments sharing a baseline into rows.
            let mut rows: Vec<(f32, f32, Vec<(f32, String, bool)>)> = vec![];
            for l in &lines {
                let mid = (l.y0 + l.y1) / 2.0;
                let h = (l.y1 - l.y0).max(1.0);
                let isbold = l.glyphs.iter().filter(|g| g.bold).count() * 2 > l.glyphs.len();
                let cells: Vec<(f32, String, bool)> = l.cells().into_iter().map(|(x, s)| (x, s, isbold)).collect();
                match rows.iter_mut().find(|r| (r.0 - mid).abs() < h * 0.5) {
                    Some(r) => r.2.extend(cells),
                    None => rows.push((mid, h, cells)),
                }
            }
            rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            // Column anchors: cluster cell start x positions.
            let mut xs: Vec<f32> = rows.iter().flat_map(|r| r.2.iter().map(|c| c.0)).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut anchors: Vec<f32> = vec![];
            for x in xs {
                if anchors.last().map(|a| x - a > 12.0).unwrap_or(true) {
                    anchors.push(x);
                }
            }
            let ws = wb.add_worksheet();
            ws.set_name(format!("Page {}", p + 1)).map_err(|e| EngineError::new("xlsx", e.to_string()))?;
            for (ri, row) in rows.iter().enumerate() {
                let mut cells = row.2.clone();
                cells.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let mut used = std::collections::HashSet::new();
                for (x, s, b) in cells {
                    let mut col = anchors.iter().rposition(|a| *a <= x + 6.0).unwrap_or(0);
                    while used.contains(&col) {
                        col += 1;
                    }
                    used.insert(col);
                    let num = s.replace([',', '$', '€', '£', '%'], "").trim().parse::<f64>();
                    let (r, c) = (ri as u32, col as u16);
                    let res = match (num, b) {
                        (Ok(v), false) if !s.starts_with('0') || s.contains('.') || s == "0" => ws.write_number(r, c, v),
                        (_, true) => ws.write_string_with_format(r, c, &s, &bold),
                        _ => ws.write_string(r, c, &s),
                    };
                    res.map_err(|e| EngineError::new("xlsx", e.to_string()))?;
                }
            }
            ws.autofit();
        }
        wb.save(out).map_err(|e| EngineError::new("xlsx", e.to_string()))?;
        Ok(())
    }

    /// Render pages to PNG or JPEG files. Returns written paths.
    pub fn export_images(&self, id: u32, pages: Vec<u32>, dpi: f32, format: &str, out_dir: &Path, stem: &str) -> Result<Vec<String>> {
        let mut written = vec![];
        let scale = dpi / 72.0;
        for p in pages {
            let img = self.render_image(id, p, scale, true)?;
            let (ext, fmt) = match format {
                "jpg" | "jpeg" => ("jpg", image::ImageFormat::Jpeg),
                "webp" => ("webp", image::ImageFormat::WebP),
                _ => ("png", image::ImageFormat::Png),
            };
            let path: PathBuf = out_dir.join(format!("{stem}_page{:03}.{ext}", p + 1));
            if fmt == image::ImageFormat::Jpeg {
                let f = std::fs::File::create(&path)?;
                let mut w = std::io::BufWriter::new(f);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 90).encode_image(&img)?;
            } else {
                img.save_with_format(&path, fmt)?;
            }
            written.push(path.to_string_lossy().to_string());
        }
        Ok(written)
    }
}
