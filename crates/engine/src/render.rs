use crate::{Result, State};
use pdfium_render::prelude::*;

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct RenderOpts {
    pub scale: f32,
    #[serde(default = "yes")]
    pub annotations: bool,
    #[serde(default = "yes")]
    pub forms: bool,
}

fn yes() -> bool {
    true
}

impl State {
    /// Render a page and return a BMP (32-bit, top-down). BMP is used because
    /// the payload is the raw BGRA buffer plus a 54-byte header, so there is
    /// no encode cost on the engine thread; webviews decode it natively.
    pub fn render_bmp(&self, id: u32, index: u32, opts: RenderOpts) -> Result<Vec<u8>> {
        let page = self.page(id, index)?;
        let scale = opts.scale.clamp(0.02, 16.0);
        // Hard cap of ~60 MP per tile to stay well inside memory limits.
        let px = page.width().value * scale * page.height().value * scale;
        let scale = if px > 60_000_000.0 { scale * (60_000_000.0 / px).sqrt() } else { scale };
        let cfg = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .render_annotations(opts.annotations)
            .render_form_data(opts.forms)
            .set_format(PdfBitmapFormat::BGRA)
            .set_reverse_byte_order(false)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE);
        let bmp = page.render_with_config(&cfg)?;
        let (w, h) = (bmp.width(), bmp.height());
        let raw = bmp.as_raw_bytes();
        Ok(encode_bmp(w as u32, h as u32, &raw))
    }

    pub fn render_image(&self, id: u32, index: u32, scale: f32, annotations: bool) -> Result<image::RgbImage> {
        let page = self.page(id, index)?;
        let cfg = PdfRenderConfig::new()
            .scale_page_by_factor(scale.clamp(0.02, 16.0))
            .render_annotations(annotations)
            .render_form_data(true)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE)
            .use_print_quality(true);
        let bmp = page.render_with_config(&cfg)?;
        Ok(bmp.as_image().to_rgb8())
    }

    /// PNG of a page, used for OCR input in the UI and image export.
    pub fn render_png(&self, id: u32, index: u32, scale: f32) -> Result<Vec<u8>> {
        let img = self.render_image(id, index, scale, false)?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)?;
        Ok(out.into_inner())
    }
}

pub fn encode_bmp(w: u32, h: u32, bgra: &[u8]) -> Vec<u8> {
    let stride = (w * 4) as usize;
    let row_bytes_in = if h > 0 { bgra.len() / h as usize } else { stride };
    let data_len = stride * h as usize;
    let mut out = Vec::with_capacity(54 + data_len);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes()); // top-down
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    if row_bytes_in == stride {
        out.extend_from_slice(&bgra[..data_len.min(bgra.len())]);
    } else {
        for row in 0..h as usize {
            let s = row * row_bytes_in;
            out.extend_from_slice(&bgra[s..s + stride]);
        }
    }
    out
}
