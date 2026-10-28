//! Conversion between PDF user space (bottom-left origin, unrotated) and
//! view space (top-left origin of the displayed, rotated page).
//!
//! PDFium's own device<->page mapping is used so crop boxes, odd origins and
//! /Rotate are handled exactly as the renderer handles them. A virtual device
//! at 64x keeps integer rounding error under 1/64 pt.

use crate::types::Rect;
use pdfium_render::prelude::*;

const K: f32 = 64.0;

fn cfg() -> PdfRenderConfig {
    PdfRenderConfig::new().scale_page_by_factor(K)
}

pub struct Mapper<'p, 'a> {
    page: &'p PdfPage<'a>,
    cfg: PdfRenderConfig,
}

impl<'p, 'a> Mapper<'p, 'a> {
    pub fn new(page: &'p PdfPage<'a>) -> Self {
        Mapper { page, cfg: cfg() }
    }

    /// View point -> PDF user space point.
    pub fn to_page(&self, x: f32, y: f32) -> (f32, f32) {
        match self.page.pixels_to_points((x * K).round() as Pixels, (y * K).round() as Pixels, &self.cfg) {
            Ok((a, b)) => (a.value, b.value),
            Err(_) => (x, self.page.height().value - y),
        }
    }

    /// PDF user space point -> view point.
    pub fn to_view(&self, x: f32, y: f32) -> (f32, f32) {
        match self.page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &self.cfg) {
            Ok((a, b)) => (a as f32 / K, b as f32 / K),
            Err(_) => (x, self.page.height().value - y),
        }
    }

    pub fn rect_to_view(&self, r: &PdfRect) -> Rect {
        let pts = [
            self.to_view(r.left().value, r.top().value),
            self.to_view(r.right().value, r.bottom().value),
            self.to_view(r.left().value, r.bottom().value),
            self.to_view(r.right().value, r.top().value),
        ];
        bbox(&pts)
    }

    pub fn quad_to_view(&self, q: &PdfQuadPoints) -> Rect {
        let pts = [
            self.to_view(q.x1.value, q.y1.value),
            self.to_view(q.x2.value, q.y2.value),
            self.to_view(q.x3.value, q.y3.value),
            self.to_view(q.x4.value, q.y4.value),
        ];
        bbox(&pts)
    }

    /// View rect -> axis-aligned PDF rect.
    pub fn rect_to_page(&self, r: &Rect) -> PdfRect {
        let r = r.normalized();
        let a = self.to_page(r.x, r.y);
        let b = self.to_page(r.right(), r.bottom());
        let (l, rt) = (a.0.min(b.0), a.0.max(b.0));
        let (bt, t) = (a.1.min(b.1), a.1.max(b.1));
        PdfRect::new_from_values(bt, l, t, rt)
    }

    /// Quad points (for markup annotations) covering a view rect, ordered
    /// as PDF expects: upper-left, upper-right, lower-left, lower-right in
    /// *reading* orientation of the displayed page.
    pub fn rect_to_quad(&self, r: &Rect) -> PdfQuadPoints {
        let r = r.normalized();
        let ul = self.to_page(r.x, r.y);
        let ur = self.to_page(r.right(), r.y);
        let ll = self.to_page(r.x, r.bottom());
        let lr = self.to_page(r.right(), r.bottom());
        PdfQuadPoints::new_from_values(ul.0, ul.1, ur.0, ur.1, ll.0, ll.1, lr.0, lr.1)
    }

    /// Length in view units -> points (scale is 1:1; kept for clarity).
    pub fn len(&self, v: f32) -> f32 {
        v
    }

    pub fn rotation(&self) -> u32 {
        match self.page.rotation().unwrap_or(PdfPageRenderRotation::None) {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 90,
            PdfPageRenderRotation::Degrees180 => 180,
            PdfPageRenderRotation::Degrees270 => 270,
        }
    }
}

fn bbox(pts: &[(f32, f32)]) -> Rect {
    let minx = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let maxx = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let miny = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let maxy = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    Rect { x: minx, y: miny, w: maxx - minx, h: maxy - miny }
}

pub fn pdf_rect_union(a: &PdfRect, b: &PdfRect) -> PdfRect {
    PdfRect::new_from_values(
        a.bottom().value.min(b.bottom().value),
        a.left().value.min(b.left().value),
        a.top().value.max(b.top().value),
        a.right().value.max(b.right().value),
    )
}
