use serde::{Deserialize, Serialize};

/// Rectangle in view space (top-left origin, y down, points).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x - 0.01 && o.y >= self.y - 0.01 && o.right() <= self.right() + 0.01 && o.bottom() <= self.bottom() + 0.01
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect { x, y, w: self.right().max(o.right()) - x, h: self.bottom().max(o.bottom()) - y }
    }
    pub fn normalized(self) -> Rect {
        let (x, w) = if self.w < 0.0 { (self.x + self.w, -self.w) } else { (self.x, self.w) };
        let (y, h) = if self.h < 0.0 { (self.y + self.h, -self.h) } else { (self.y, self.h) };
        Rect { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageInfo {
    pub index: u32,
    /// Display width/height in points (rotation applied).
    pub width: f32,
    pub height: f32,
    pub rotation: u32,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocInfo {
    pub id: u32,
    pub name: String,
    pub path: Option<String>,
    pub page_count: u32,
    pub pages: Vec<PageInfo>,
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
    pub created: String,
    pub modified: String,
    pub version: String,
    pub has_forms: bool,
    pub protected: bool,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub children: Vec<OutlineItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextChar {
    pub c: String,
    pub r: Rect,
    /// Synthesised by PDFium (spaces / line breaks inferred from layout).
    pub g: bool,
    pub size: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub page: u32,
    pub rects: Vec<Rect>,
    pub context: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotInfo {
    pub index: u32,
    pub kind: String,
    pub rect: Rect,
    pub color: String,
    pub contents: String,
    pub author: String,
    pub modified: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldInfo {
    pub page: u32,
    pub annot_index: u32,
    pub name: String,
    pub kind: String,
    pub rect: Rect,
    pub value: String,
    pub checked: bool,
    pub options: Vec<String>,
    pub read_only: bool,
    pub required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjInfo {
    pub index: u32,
    pub kind: String,
    pub rect: Rect,
    pub text: Option<String>,
    pub font_size: f32,
    pub font_name: String,
    pub color: String,
}

/// Colour in `#rrggbb` or `#rrggbbaa`.
pub fn parse_color(s: &str) -> pdfium_render::prelude::PdfColor {
    let h = s.trim().trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    let a = if h.len() >= 8 { p(6) } else { 255 };
    pdfium_render::prelude::PdfColor::new(p(0), p(2), p(4), a)
}

pub fn color_hex(c: &pdfium_render::prelude::PdfColor) -> String {
    format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
}
