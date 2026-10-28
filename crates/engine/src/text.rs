use crate::geom::Mapper;
use crate::types::{Rect, SearchHit, TextChar};
use crate::{Result, State};
use pdfium_render::prelude::*;

impl State {
    /// Every character on the page in content order with its view-space box.
    pub fn page_chars(&self, id: u32, index: u32) -> Result<Vec<TextChar>> {
        let page = self.page(id, index)?;
        let map = Mapper::new(&page);
        let text = page.text()?;
        let mut out = Vec::with_capacity(text.len().max(0) as usize);
        for ch in text.chars().iter() {
            let s = ch.unicode_string().unwrap_or_default();
            if s.is_empty() {
                continue;
            }
            let generated = ch.is_generated().unwrap_or(false);
            let r = match ch.loose_bounds() {
                Ok(b) => map.rect_to_view(&b),
                Err(_) => Rect::default(),
            };
            out.push(TextChar { c: s, r, g: generated, size: ch.scaled_font_size().value });
        }
        Ok(out)
    }

    pub fn page_text(&self, id: u32, index: u32) -> Result<String> {
        let page = self.page(id, index)?;
        let text = page.text()?;
        Ok(text.all())
    }

    pub fn page_has_text(&self, id: u32, index: u32) -> Result<bool> {
        let page = self.page(id, index)?;
        let text = page.text()?;
        Ok(text.all().chars().any(|c| !c.is_whitespace()))
    }

    pub fn search(&self, id: u32, query: &str, match_case: bool, whole_word: bool, limit: usize) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        let doc = self.doc(id)?;
        let opts = PdfSearchOptions::new().match_case(match_case).match_whole_word(whole_word);
        let mut hits = Vec::new();
        for (pi, page) in doc.pdf.pages().iter().enumerate() {
            let map = Mapper::new(&page);
            let text = page.text()?;
            let search = match text.search(query, &opts) {
                Ok(s) => s,
                Err(_) => continue,
            };
            while let Some(segs) = search.find_next() {
                let mut rects = Vec::new();
                let mut ctx = String::new();
                for seg in segs.iter() {
                    rects.push(map.rect_to_view(&seg.bounds()));
                    // Pull a little surrounding text for the result list.
                    let b = seg.bounds();
                    let wide = PdfRect::new_from_values(
                        b.bottom().value,
                        (b.left().value - 120.0).max(0.0),
                        b.top().value,
                        b.right().value + 120.0,
                    );
                    if ctx.is_empty() {
                        ctx = text.inside_rect(wide).replace(['\r', '\n'], " ");
                    }
                }
                hits.push(SearchHit { page: pi as u32, rects, context: ctx.trim().to_string() });
                if hits.len() >= limit {
                    return Ok(hits);
                }
            }
        }
        Ok(hits)
    }
}
