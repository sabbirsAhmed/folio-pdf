//! True redaction: content under the marked areas is removed from the file,
//! not just covered. Text objects are rebuilt character-by-character so text
//! outside the area survives; image pixels under the area are overwritten;
//! vector paths fully inside are removed; overlapping annotations are deleted.

use crate::geom::Mapper;
use crate::types::{parse_color, Rect};
use crate::{Result, State};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
pub struct RedactArea {
    pub page: u32,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RedactReport {
    pub text_objects_rebuilt: u32,
    pub text_objects_removed: u32,
    pub images_masked: u32,
    pub paths_removed: u32,
    pub annotations_removed: u32,
    /// Form XObjects overlapping an area that were removed wholesale.
    pub forms_removed: u32,
}

struct Run {
    text: String,
    origin: (f32, f32),
}

fn pdf_rect_intersects(a: &PdfRect, b: &PdfRect) -> bool {
    a.left().value < b.right().value && b.left().value < a.right().value && a.bottom().value < b.top().value && b.bottom().value < a.top().value
}

fn pdf_rect_contains(outer: &PdfRect, inner: &PdfRect) -> bool {
    inner.left().value >= outer.left().value - 0.5
        && inner.right().value <= outer.right().value + 0.5
        && inner.bottom().value >= outer.bottom().value - 0.5
        && inner.top().value <= outer.top().value + 0.5
}

fn quad_to_rect(q: &PdfQuadPoints) -> PdfRect {
    PdfRect::new_from_values(q.bottom().value, q.left().value, q.top().value, q.right().value)
}

impl State {
    pub fn apply_redactions(&mut self, id: u32, areas: Vec<RedactArea>, fill: String) -> Result<RedactReport> {
        self.checkpoint(id)?;
        let mut by_page: BTreeMap<u32, Vec<Rect>> = BTreeMap::new();
        for a in areas {
            by_page.entry(a.page).or_default().push(a.rect.normalized());
        }
        let fill = parse_color(&fill);
        let mut rep = RedactReport::default();
        for (pi, rects) in by_page {
            self.redact_page(id, pi, &rects, fill, &mut rep)?;
        }
        Ok(rep)
    }

    fn redact_page(&mut self, id: u32, pi: u32, view_rects: &[Rect], fill: PdfColor, rep: &mut RedactReport) -> Result<()> {
        let mut page = self.page(id, pi)?;
        let map_page = self.page(id, pi)?;
        let map = Mapper::new(&map_page);
        let rects: Vec<PdfRect> = view_rects.iter().map(|r| map.rect_to_page(r)).collect();
        let hits = |b: &PdfRect| rects.iter().any(|r| pdf_rect_intersects(r, b));

        // 1. Plan text rebuilds while the text page is alive.
        struct TextPlan {
            index: usize,
            runs: Vec<Run>,
        }
        let mut text_plans: Vec<TextPlan> = vec![];
        let mut remove: Vec<usize> = vec![];
        let mut image_masks: Vec<(usize, Vec<PdfRect>)> = vec![];
        {
            let text = page.text()?;
            for (i, o) in page.objects().iter().enumerate() {
                let Ok(q) = o.bounds() else { continue };
                let b = quad_to_rect(&q);
                if !hits(&b) {
                    continue;
                }
                match o.object_type() {
                    PdfPageObjectType::Text => {
                        let t = o.as_text_object().unwrap();
                        let mut runs: Vec<Run> = vec![];
                        let mut cur: Option<Run> = None;
                        let mut any_removed = false;
                        if let Ok(chars) = text.chars_for_object(t) {
                            for ch in chars.iter() {
                                let s = ch.unicode_string().unwrap_or_default();
                                let cb = ch.loose_bounds().unwrap_or(PdfRect::ZERO);
                                let gone = rects.iter().any(|r| {
                                    // Remove a glyph if its centre lies in the area.
                                    let cx = (cb.left().value + cb.right().value) / 2.0;
                                    let cy = (cb.bottom().value + cb.top().value) / 2.0;
                                    cx >= r.left().value && cx <= r.right().value && cy >= r.bottom().value && cy <= r.top().value
                                });
                                if gone {
                                    any_removed = true;
                                    if let Some(r) = cur.take() {
                                        runs.push(r);
                                    }
                                    continue;
                                }
                                let origin = ch.origin().map(|(x, y)| (x.value, y.value)).unwrap_or((cb.left().value, cb.bottom().value));
                                match &mut cur {
                                    Some(r) => r.text.push_str(&s),
                                    None => {
                                        if !s.trim().is_empty() {
                                            cur = Some(Run { text: s, origin });
                                        }
                                    }
                                }
                            }
                            if let Some(r) = cur.take() {
                                runs.push(r);
                            }
                        } else {
                            any_removed = true;
                        }
                        if any_removed {
                            text_plans.push(TextPlan { index: i, runs: runs.into_iter().filter(|r| !r.text.trim().is_empty()).collect() });
                        }
                    }
                    PdfPageObjectType::Image => {
                        let covering: Vec<PdfRect> = rects.iter().filter(|r| pdf_rect_intersects(r, &b)).copied().collect();
                        if covering.iter().any(|r| pdf_rect_contains(r, &b)) {
                            remove.push(i);
                        } else {
                            image_masks.push((i, covering));
                        }
                    }
                    PdfPageObjectType::Path | PdfPageObjectType::Shading => {
                        if rects.iter().any(|r| pdf_rect_contains(r, &b)) {
                            remove.push(i);
                            rep.paths_removed += 1;
                        }
                    }
                    PdfPageObjectType::XObjectForm => {
                        remove.push(i);
                        rep.forms_removed += 1;
                    }
                    _ => {}
                }
            }
        }

        // 2. Rebuild text objects: insert surviving runs, then drop the original.
        let doc = &self.doc(id)?.pdf;
        let mut inserts: Vec<(usize, Vec<PdfPageTextObject<'static>>)> = vec![];
        for plan in &text_plans {
            let o = page.objects().get(plan.index as PdfPageObjectIndex)?;
            let t = o.as_text_object().unwrap();
            let size = t.unscaled_font_size();
            let m = o.matrix()?;
            let color = o.fill_color().unwrap_or(PdfColor::BLACK);
            let mode = t.render_mode();
            let mut objs = vec![];
            for run in &plan.runs {
                let mut nt = PdfPageTextObject::new(doc, &run.text, t.font(), size)?;
                // Keep scale/rotation/skew, move to the run's first glyph origin.
                let mm = PdfMatrix::new(m.a(), m.b(), m.c(), m.d(), run.origin.0, run.origin.1);
                nt.apply_matrix(mm)?;
                nt.set_fill_color(color)?;
                if mode != PdfPageTextRenderMode::Unknown {
                    let _ = nt.set_render_mode(mode);
                }
                objs.push(nt);
            }
            if objs.is_empty() {
                rep.text_objects_removed += 1;
            } else {
                rep.text_objects_rebuilt += 1;
            }
            inserts.push((plan.index, objs));
            remove.push(plan.index);
        }

        // 3. Mask image pixels.
        for (i, cover) in &image_masks {
            let mut o = page.objects().get(*i as PdfPageObjectIndex)?;
            let q = o.bounds()?;
            let b = quad_to_rect(&q);
            let Some(img_obj) = o.as_image_object_mut() else { continue };
            let Ok(img) = img_obj.get_raw_image() else {
                remove.push(*i);
                continue;
            };
            let mut rgba = img.to_rgba8();
            let (iw, ih) = (rgba.width() as f32, rgba.height() as f32);
            let (bw, bh) = (b.width().value.max(0.01), b.height().value.max(0.01));
            for r in cover {
                let x0 = (((r.left().value - b.left().value) / bw) * iw).floor().clamp(0.0, iw) as u32;
                let x1 = (((r.right().value - b.left().value) / bw) * iw).ceil().clamp(0.0, iw) as u32;
                let y0 = (((b.top().value - r.top().value) / bh) * ih).floor().clamp(0.0, ih) as u32;
                let y1 = (((b.top().value - r.bottom().value) / bh) * ih).ceil().clamp(0.0, ih) as u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        rgba.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                    }
                }
            }
            img_obj.set_image(&image::DynamicImage::ImageRgba8(rgba))?;
            rep.images_masked += 1;
        }

        // 4. Remove originals (descending) and insert rebuilt runs in their place.
        remove.sort_unstable();
        remove.dedup();
        let mut inserts: BTreeMap<usize, Vec<PdfPageTextObject<'static>>> = inserts.into_iter().collect();
        for idx in remove.into_iter().rev() {
            let removed = page.objects_mut().remove_object_at_index(idx as PdfPageObjectIndex)?;
            // Rebuilt runs share the removed object's font; destroying the
            // detached original can free state they still reference inside
            // PDFium, so it is intentionally leaked (bytes, per redaction).
            std::mem::forget(removed);
            if let Some(objs) = inserts.remove(&idx) {
                for (k, o) in objs.into_iter().enumerate() {
                    page.objects_mut().insert_object_at_index((idx + k) as PdfPageObjectIndex, PdfPageObject::from(o))?;
                }
            }
        }

        // 5. Annotations touching the area go too (comments can carry text).
        let n = page.annotations().len();
        for ai in (0..n).rev() {
            let a = page.annotations().get(ai)?;
            if a.annotation_type() == PdfPageAnnotationType::Widget {
                continue;
            }
            if let Ok(b) = a.bounds() {
                if hits(&b) {
                    page.annotations_mut().delete_annotation(a)?;
                    rep.annotations_removed += 1;
                }
            }
        }

        // 6. Paint the redaction boxes.
        for r in &rects {
            let path = PdfPagePathObject::new_rect(doc, *r, None, None, Some(fill))?;
            page.objects_mut().add_path_object(path)?;
        }
        page.regenerate_content()?;
        Ok(())
    }
}
