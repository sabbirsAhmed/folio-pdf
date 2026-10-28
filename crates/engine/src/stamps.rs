//! Helpers that build page objects oriented to the *displayed* page, so text
//! and images come out upright on pages carrying a /Rotate entry.

use crate::geom::Mapper;
use crate::types::Rect;
use crate::Result;
use pdfium_render::prelude::*;

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FontFamily {
    Helvetica,
    Times,
    Courier,
}

impl Default for FontFamily {
    fn default() -> Self {
        FontFamily::Helvetica
    }
}

pub fn builtin_font(doc: &mut PdfDocument<'static>, family: FontFamily, bold: bool, italic: bool) -> PdfFontToken {
    let f = doc.fonts_mut();
    match (family, bold, italic) {
        (FontFamily::Helvetica, false, false) => f.helvetica(),
        (FontFamily::Helvetica, true, false) => f.helvetica_bold(),
        (FontFamily::Helvetica, false, true) => f.helvetica_oblique(),
        (FontFamily::Helvetica, true, true) => f.helvetica_bold_oblique(),
        (FontFamily::Times, false, false) => f.times_roman(),
        (FontFamily::Times, true, false) => f.times_bold(),
        (FontFamily::Times, false, true) => f.times_italic(),
        (FontFamily::Times, true, true) => f.times_bold_italic(),
        (FontFamily::Courier, false, false) => f.courier(),
        (FontFamily::Courier, true, false) => f.courier_bold(),
        (FontFamily::Courier, false, true) => f.courier_oblique(),
        (FontFamily::Courier, true, true) => f.courier_bold_oblique(),
    }
}

/// Approximate advance width of `s` in a standard font, in points.
/// Used for centring/right-aligning text before PDFium has laid it out.
pub fn approx_width(s: &str, size: f32, family: FontFamily, bold: bool) -> f32 {
    if family == FontFamily::Courier {
        return s.chars().count() as f32 * 0.6 * size;
    }
    let mut w = 0.0;
    for c in s.chars() {
        let em = match c {
            'i' | 'j' | 'l' | '.' | ',' | '\'' | '!' | '|' | ':' | ';' => 0.28,
            'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '[' | ']' | '-' => 0.33,
            'm' | 'w' | 'M' | 'W' => 0.83,
            'A'..='Z' => 0.68,
            '0'..='9' => 0.556,
            _ => 0.52,
        };
        w += em;
    }
    let w = w * size;
    if bold { w * 1.06 } else { w }
}

/// Create a text object whose baseline starts at view point (x, y), upright
/// relative to the displayed page, optionally rotated further by `extra_deg`
/// (counter-clockwise, as seen on screen).
#[allow(clippy::too_many_arguments)]
pub fn text_object(
    doc: &PdfDocument<'static>,
    map: &Mapper,
    text: &str,
    font: PdfFontToken,
    size: f32,
    color: PdfColor,
    view_x: f32,
    view_y: f32,
    extra_deg: f32,
) -> Result<PdfPageTextObject<'static>> {
    let safe = if text.is_empty() { " " } else { text };
    let mut obj = PdfPageTextObject::new(doc, safe, font, PdfPoints::new(size))?;
    obj.set_fill_color(color)?;
    let rot = map.rotation() as f32;
    let total = rot + extra_deg;
    if total.abs() > 0.001 {
        obj.rotate_counter_clockwise_degrees(total)?;
    }
    let (px, py) = map.to_page(view_x, view_y);
    obj.translate(PdfPoints::new(px), PdfPoints::new(py))?;
    Ok(obj)
}

/// Create an image object filling the given view rect, upright on screen.
pub fn image_object(
    doc: &PdfDocument<'static>,
    map: &Mapper,
    img: &image::DynamicImage,
    rect: &Rect,
) -> Result<PdfPageImageObject<'static>> {
    let rot = map.rotation();
    // Pre-rotate pixels so that after the page's display rotation the image
    // appears upright. Page rotates content clockwise by `rot` on display, so
    // content must be rotated counter-clockwise by `rot` in page space.
    let img = match rot {
        90 => img.rotate270(),
        180 => img.rotate180(),
        270 => img.rotate90(),
        _ => img.clone(),
    };
    let pr = map.rect_to_page(rect);
    let mut obj = PdfPageImageObject::new_with_size(doc, &img, pr.width(), pr.height())?;
    obj.translate(pr.left(), pr.bottom())?;
    Ok(obj)
}

/// A closed rectangular path in view space.
pub fn rect_path(
    doc: &PdfDocument<'static>,
    map: &Mapper,
    rect: &Rect,
    stroke: Option<(PdfColor, f32)>,
    fill: Option<PdfColor>,
) -> Result<PdfPagePathObject<'static>> {
    let r = rect.normalized();
    let p0 = map.to_page(r.x, r.y);
    let p1 = map.to_page(r.right(), r.y);
    let p2 = map.to_page(r.right(), r.bottom());
    let p3 = map.to_page(r.x, r.bottom());
    let mut path = PdfPagePathObject::new(
        doc,
        PdfPoints::new(p0.0),
        PdfPoints::new(p0.1),
        stroke.map(|s| s.0),
        stroke.map(|s| PdfPoints::new(s.1)),
        fill,
    )?;
    path.line_to(PdfPoints::new(p1.0), PdfPoints::new(p1.1))?;
    path.line_to(PdfPoints::new(p2.0), PdfPoints::new(p2.1))?;
    path.line_to(PdfPoints::new(p3.0), PdfPoints::new(p3.1))?;
    path.close_path()?;
    Ok(path)
}

/// An open or closed polyline in view space.
pub fn polyline_path(
    doc: &PdfDocument<'static>,
    map: &Mapper,
    pts: &[(f32, f32)],
    color: PdfColor,
    width: f32,
    closed: bool,
    fill: Option<PdfColor>,
) -> Result<PdfPagePathObject<'static>> {
    let first = pts.first().copied().unwrap_or((0.0, 0.0));
    let p0 = map.to_page(first.0, first.1);
    let mut path = PdfPagePathObject::new(
        doc,
        PdfPoints::new(p0.0),
        PdfPoints::new(p0.1),
        Some(color),
        Some(PdfPoints::new(width)),
        fill,
    )?;
    if pts.len() == 1 {
        // A dot: tiny segment so it renders with round caps.
        path.line_to(PdfPoints::new(p0.0 + 0.1), PdfPoints::new(p0.1))?;
    }
    for p in pts.iter().skip(1) {
        let q = map.to_page(p.0, p.1);
        path.line_to(PdfPoints::new(q.0), PdfPoints::new(q.1))?;
    }
    if closed {
        path.close_path()?;
    }
    path.set_line_cap(PdfPageObjectLineCap::Round)?;
    path.set_line_join(PdfPageObjectLineJoin::Round)?;
    Ok(path)
}

/// Sample an ellipse inscribed in a view rect as a polyline.
pub fn ellipse_points(r: &Rect, n: usize) -> Vec<(f32, f32)> {
    let r = r.normalized();
    let (cx, cy, rx, ry) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect()
}
