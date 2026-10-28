//! End-to-end checks against a real PDFium build.
//! Set FOLIO_PDFIUM_DIR to the folder holding the PDFium shared library.

use folio_engine::annots::{MarkupStyle, NewAnnot, ShapeKind};
use folio_engine::edit::{HeaderFooter, NewText, OcrWord, Watermark};
use folio_engine::forms::{FieldValue, NewField};
use folio_engine::lo::{CompressLevel, MetadataUpdate, SanitizeOpts};
use folio_engine::redact::RedactArea;
use folio_engine::render::RenderOpts;
use folio_engine::*;
use std::path::PathBuf;

/// PDFium may be initialised once per process, so all tests share one engine.
fn engine() -> Engine {
    static E: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();
    E.get_or_init(|| {
        let dir = std::env::var("FOLIO_PDFIUM_DIR").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium-bundle")
        });
        Engine::start(vec![dir]).expect("engine start")
    })
    .clone()
}

fn out_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn snapshot(e: &Engine, id: u32, page: u32, name: &str) {
    let png = e.exec(move |s| s.render_png(id, page, 1.5)).unwrap();
    std::fs::write(out_dir().join(name), png).unwrap();
}

fn text_of(e: &Engine, id: u32, page: u32) -> String {
    e.exec(move |s| s.page_text(id, page)).unwrap()
}

#[test]
fn full_workflow() {
    let e = engine();
    let out = out_dir();

    // ---- create + text
    let info = e.exec(|s| s.create_blank(612.0, 792.0, 2)).unwrap();
    let id = info.id;
    assert_eq!(info.page_count, 2);
    e.exec(move |s| {
        s.add_text(id, 0, NewText { x: 72.0, y: 72.0, text: "Quarterly Report\nRevenue grew 12 percent".into(), size: 18.0, color: "#112233".into(), family: Default::default(), bold: true, italic: false })
    })
    .unwrap();
    e.exec(move |s| {
        s.add_text(id, 0, NewText { x: 72.0, y: 200.0, text: "Secret account 4417 belongs to Alice".into(), size: 12.0, color: "#000000".into(), family: Default::default(), bold: false, italic: false })
    })
    .unwrap();
    let t = text_of(&e, id, 0);
    assert!(t.contains("Quarterly Report"), "text: {t}");
    let chars = e.exec(move |s| s.page_chars(id, 0)).unwrap();
    let q = chars.iter().find(|c| c.c == "Q").unwrap();
    assert!((q.r.x - 72.0).abs() < 3.0 && (q.r.y - 72.0).abs() < 8.0, "Q at {:?}", q.r);

    // ---- search
    let hits = e.exec(move |s| s.search(id, "revenue", false, false, 100)).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 0);

    // ---- annotations
    let line_rect = hits[0].rects[0];
    let annots = vec![
        NewAnnot::Markup { style: MarkupStyle::Highlight, rects: vec![line_rect], color: "#ffeb3b".into(), note: "check".into() },
        NewAnnot::Markup { style: MarkupStyle::Underline, rects: vec![line_rect], color: "#2962ff".into(), note: String::new() },
        NewAnnot::Ink { strokes: vec![vec![[300.0, 300.0], [350.0, 320.0], [400.0, 300.0]]], color: "#e53935".into(), width: 2.0 },
        NewAnnot::Shape { shape: ShapeKind::Rect, rect: Rect { x: 300.0, y: 400.0, w: 120.0, h: 60.0 }, color: "#43a047".into(), width: 2.0, fill: None },
        NewAnnot::Shape { shape: ShapeKind::Ellipse, rect: Rect { x: 440.0, y: 400.0, w: 100.0, h: 60.0 }, color: "#8e24aa".into(), width: 2.0, fill: None },
        NewAnnot::Line { from: [80.0, 500.0], to: [250.0, 560.0], color: "#000000".into(), width: 2.0, arrow: true },
        NewAnnot::Note { x: 520.0, y: 80.0, text: "Please verify".into(), color: "#ffc107".into() },
        NewAnnot::TextBox { rect: Rect { x: 80.0, y: 600.0, w: 220.0, h: 40.0 }, text: "Typed comment that wraps across lines nicely".into(), size: 11.0, color: "#c62828".into(), family: Default::default(), bold: false, border: Some("#c62828".into()) },
        NewAnnot::Stamp { rect: Rect { x: 360.0, y: 600.0, w: 180.0, h: 50.0 }, label: "APPROVED".into(), color: "#2e7d32".into() },
    ];
    for a in annots {
        e.exec(move |s| s.add_annot(id, 0, a, "Tester")).unwrap();
    }
    let list = e.exec(move |s| s.list_annots(id, 0)).unwrap();
    assert_eq!(list.len(), 9, "{list:#?}");
    assert!(list.iter().any(|a| a.kind == "highlight" && a.contents == "check"));
    snapshot(&e, id, 0, "01-annots.png");

    // ---- undo/redo
    let before = e.exec(move |s| s.list_annots(id, 0)).unwrap().len();
    e.exec(move |s| s.undo(id)).unwrap();
    assert_eq!(e.exec(move |s| s.list_annots(id, 0)).unwrap().len(), before - 1);
    e.exec(move |s| s.redo(id)).unwrap();
    assert_eq!(e.exec(move |s| s.list_annots(id, 0)).unwrap().len(), before);

    // ---- forms
    let fields = vec![
        NewField { page: 1, kind: "text".into(), name: "full_name".into(), rect: Rect { x: 72.0, y: 100.0, w: 200.0, h: 22.0 }, options: vec![], required: true, font_size: 0.0, export_value: String::new() },
        NewField { page: 1, kind: "checkbox".into(), name: "agree".into(), rect: Rect { x: 72.0, y: 140.0, w: 16.0, h: 16.0 }, options: vec![], required: false, font_size: 0.0, export_value: String::new() },
        NewField { page: 1, kind: "combo".into(), name: "country".into(), rect: Rect { x: 72.0, y: 180.0, w: 160.0, h: 22.0 }, options: vec!["Bangladesh".into(), "Estonia".into(), "UAE".into()], required: false, font_size: 0.0, export_value: String::new() },
        NewField { page: 1, kind: "radio".into(), name: "plan".into(), rect: Rect { x: 72.0, y: 220.0, w: 14.0, h: 14.0 }, options: vec![], required: false, font_size: 0.0, export_value: "basic".into() },
        NewField { page: 1, kind: "radio".into(), name: "plan".into(), rect: Rect { x: 120.0, y: 220.0, w: 14.0, h: 14.0 }, options: vec![], required: false, font_size: 0.0, export_value: "pro".into() },
        NewField { page: 1, kind: "multiline".into(), name: "notes".into(), rect: Rect { x: 72.0, y: 260.0, w: 300.0, h: 80.0 }, options: vec![], required: false, font_size: 10.0, export_value: String::new() },
    ];
    for f in fields {
        e.exec(move |s| s.add_field(id, f)).unwrap();
    }
    let ff = e.exec(move |s| s.list_fields(id, 1)).unwrap();
    assert_eq!(ff.len(), 6, "{ff:#?}");
    let by = |n: &str| ff.iter().find(|f| f.name == n).cloned().unwrap();
    let (name, agree, country) = (by("full_name"), by("agree"), by("country"));
    assert_eq!(country.options.len(), 3);
    e.exec(move |s| s.set_field(id, 1, name.annot_index, FieldValue::Text { value: "Rahim Uddin".into() })).unwrap();
    e.exec(move |s| s.set_field(id, 1, agree.annot_index, FieldValue::Check { checked: true })).unwrap();
    e.exec(move |s| s.set_field(id, 1, country.annot_index, FieldValue::Choice { value: "Estonia".into() })).unwrap();
    let radios: Vec<_> = e.exec(move |s| s.list_fields(id, 1)).unwrap().into_iter().filter(|f| f.kind == "radio").collect();
    assert_eq!(radios.len(), 2);
    let pro = radios[1].annot_index;
    e.exec(move |s| s.set_field(id, 1, pro, FieldValue::Check { checked: true })).unwrap();
    let data = e.exec(move |s| s.export_form_data(id)).unwrap();
    assert_eq!(data["full_name"], "Rahim Uddin");
    assert_eq!(data["agree"], true);
    assert_eq!(data["country"], "Estonia");
    snapshot(&e, id, 1, "02-form-filled.png");
    // Round-trip import
    let mut d2 = data.clone();
    d2["full_name"] = serde_json::json!("Karim");
    e.exec(move |s| s.import_form_data(id, d2)).unwrap();
    assert_eq!(e.exec(move |s| s.export_form_data(id)).unwrap()["full_name"], "Karim");

    // ---- save + reopen keeps fields and annotations
    let p1 = out.join("workflow.pdf");
    let p1c = p1.clone();
    e.exec(move |s| s.save(id, Some(p1c))).unwrap();
    let p1c = p1.clone();
    let r = e.exec(move |s| s.open(&p1c, None)).unwrap();
    assert_eq!(e.exec(move |s| s.list_fields(r.id, 1)).unwrap().len(), 6);
    assert_eq!(e.exec(move |s| s.list_annots(r.id, 0)).unwrap().len(), 9);
    assert_eq!(e.exec(move |s| s.export_form_data(r.id)).unwrap()["full_name"], "Karim");

    // ---- flatten
    e.exec(move |s| s.flatten(r.id)).unwrap();
    assert_eq!(e.exec(move |s| s.list_fields(r.id, 1)).unwrap().len(), 0);
    assert!(text_of(&e, r.id, 1).contains("Karim"), "flattened text: {}", text_of(&e, r.id, 1));
    snapshot(&e, r.id, 1, "03-flattened.png");

    // ---- redaction: remove "4417" but keep the rest of the line
    let hit = e.exec(move |s| s.search(id, "4417", false, false, 10)).unwrap();
    let area = RedactArea { page: 0, rect: hit[0].rects[0] };
    let rep = e.exec(move |s| s.apply_redactions(id, vec![area], "#000000".into())).unwrap();
    let t = text_of(&e, id, 0);
    assert!(!t.contains("4417"), "redacted text still present: {t}");
    assert!(t.contains("Secret account") && t.contains("Alice"), "lost surrounding text: {t} {rep:?}");
    snapshot(&e, id, 0, "04-redacted.png");

    // ---- edit object text
    let objs = e.exec(move |s| s.list_objects(id, 0, false)).unwrap();
    let title = objs.iter().find(|o| o.text.as_deref() == Some("Quarterly Report")).unwrap().index;
    e.exec(move |s| s.set_object_text(id, 0, title, "Annual Report".into())).unwrap();
    assert!(text_of(&e, id, 0).contains("Annual Report"));
    e.exec(move |s| s.move_object(id, 0, title, 10.0, 0.0)).unwrap();

    // ---- watermark + header/footer
    e.exec(move |s| s.watermark(id, Watermark { text: "CONFIDENTIAL".into(), size: 60.0, color: "#ef9a9a".into(), angle: 45.0, pages: String::new(), behind: true })).unwrap();
    e.exec(move |s| {
        s.header_footer(
            id,
            HeaderFooter { top_left: String::new(), top_center: "{name}".into(), top_right: String::new(), bottom_left: String::new(), bottom_center: "Page {page} of {total}".into(), bottom_right: String::new(), size: 9.0, color: "#555555".into(), margin: 36.0, start_number: 1, pages: String::new(), date: "2026-10-27".into() },
            "Report".into(),
        )
    })
    .unwrap();
    assert!(text_of(&e, id, 1).contains("Page 2 of 2"));
    snapshot(&e, id, 0, "05-watermark.png");

    // ---- page operations
    e.exec(move |s| s.insert_blank(id, 2, 612.0, 792.0)).unwrap();
    e.exec(move |s| s.add_text(id, 2, NewText { x: 72.0, y: 72.0, text: "THIRD".into(), size: 20.0, color: "#000000".into(), family: Default::default(), bold: false, italic: false })).unwrap();
    let info = e.exec(move |s| s.reorder_pages(id, vec![2, 0, 1])).unwrap();
    assert_eq!(info.page_count, 3);
    assert!(text_of(&e, id, 0).contains("THIRD"));
    // fields survive reordering (they are now on page index 2)
    assert_eq!(e.exec(move |s| s.list_fields(id, 2)).unwrap().len(), 6);
    let info = e.exec(move |s| s.rotate_pages(id, vec![0], 90)).unwrap();
    assert_eq!(info.pages[0].rotation, 90);
    assert!((info.pages[0].width - 792.0).abs() < 0.5, "rotated width {}", info.pages[0].width);
    // Rotated page: view-space placement must match what's rendered.
    e.exec(move |s| s.add_text(id, 0, NewText { x: 100.0, y: 100.0, text: "Upright".into(), size: 20.0, color: "#0000ff".into(), family: Default::default(), bold: false, italic: false })).unwrap();
    let ch = e.exec(move |s| s.page_chars(id, 0)).unwrap();
    let u = ch.iter().find(|c| c.c == "U").expect("U");
    assert!((u.r.x - 100.0).abs() < 4.0 && (u.r.y - 100.0).abs() < 10.0 && u.r.w < u.r.h * 1.5, "rotated placement {:?}", u.r);
    snapshot(&e, id, 0, "06-rotated.png");
    let info = e.exec(move |s| s.move_pages(id, vec![0], 3)).unwrap();
    assert_eq!(info.pages[2].rotation, 90);
    let info = e.exec(move |s| s.duplicate_pages(id, vec![0])).unwrap();
    assert_eq!(info.page_count, 4);
    let info = e.exec(move |s| s.delete_pages(id, vec![1])).unwrap();
    assert_eq!(info.page_count, 3);
    let ex = out.join("extract.pdf");
    let exc = ex.clone();
    e.exec(move |s| s.extract_pages(id, vec![0, 2], &exc, false)).unwrap();
    let exc = ex.clone();
    let exi = e.exec(move |s| s.open(&exc, None)).unwrap();
    assert_eq!(exi.page_count, 2);
    let outc = out.clone();
    let parts = e.exec(move |s| s.split(id, Some(1), None, None, &outc, "split")).unwrap();
    assert_eq!(parts.len(), 3);
    let p0 = PathBuf::from(&parts[0]);
    let p2 = PathBuf::from(&parts[2]);
    let comb = e.exec(move |s| s.combine(vec![(p0, None), (p2, None)])).unwrap();
    assert_eq!(comb.page_count, 2);
    let exc = ex.clone();
    let info = e.exec(move |s| s.insert_from_file(id, 0, &exc, "2", None)).unwrap();
    assert_eq!(info.page_count, 4);

    // ---- metadata + outline
    let bytes = e.exec(move |s| s.doc(id).map(|d| d.pdf.save_to_bytes().unwrap())).unwrap();
    let md = folio_engine::lo::set_metadata(&bytes, &MetadataUpdate { title: Some("Annual Report — 2026".into()), author: Some("Stellar".into()), ..Default::default() }).unwrap();
    let ol = folio_engine::lo::set_outline(&md, &[OutlineItem { title: "Intro".into(), page: Some(0), children: vec![OutlineItem { title: "Details".into(), page: Some(2), children: vec![] }] }]).unwrap();
    e.exec(move |s| s.replace_bytes(id, ol)).unwrap();
    let info = e.exec(move |s| s.info(id)).unwrap();
    assert_eq!(info.title, "Annual Report — 2026");
    let outline = e.exec(move |s| s.outline(id)).unwrap();
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].children[0].page, Some(2));

    // ---- OCR layer
    e.exec(move |s| s.add_ocr_layer(id, 1, vec![OcrWord { text: "Scanned".into(), x: 50.0, y: 700.0, w: 80.0, h: 14.0 }, OcrWord { text: "invoice".into(), x: 135.0, y: 700.0, w: 60.0, h: 14.0 }])).unwrap();
    let hits = e.exec(move |s| s.search(id, "invoice", false, false, 10)).unwrap();
    assert!(hits.iter().any(|h| h.page == 1), "ocr text not searchable");

    // ---- exports
    let docx = out.join("export.docx");
    let docxc = docx.clone();
    e.exec(move |s| s.export_docx(id, &docxc, true)).unwrap();
    assert!(std::fs::metadata(&docx).unwrap().len() > 2000);
    let xlsx = out.join("export.xlsx");
    let xlsxc = xlsx.clone();
    e.exec(move |s| s.export_xlsx(id, &xlsxc)).unwrap();
    assert!(std::fs::metadata(&xlsx).unwrap().len() > 2000);
    let txt = out.join("export.txt");
    let txtc = txt.clone();
    e.exec(move |s| s.export_text(id, &txtc)).unwrap();
    assert!(std::fs::read_to_string(&txt).unwrap().contains("Secret account"));
    let outc = out.clone();
    let imgs = e.exec(move |s| s.export_images(id, vec![0, 1], 96.0, "jpg", &outc, "img")).unwrap();
    assert_eq!(imgs.len(), 2);

    // ---- images -> pdf -> compress
    let big = image::RgbImage::from_fn(2400, 1800, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, ((x + y) % 255) as u8]));
    let imgp = out.join("photo.png");
    big.save(&imgp).unwrap();
    let imgi = e.exec(move |s| s.from_images(vec![imgp])).unwrap();
    let before = e.exec(move |s| s.output_bytes(imgi.id)).unwrap();
    let (after, rep) = folio_engine::lo::compress(&before, CompressLevel::Medium).unwrap();
    assert!(after.len() < before.len() / 2, "compress {} -> {} {rep:?}", before.len(), after.len());
    e.exec(move |s| s.replace_bytes(imgi.id, after)).unwrap();
    snapshot(&e, imgi.id, 0, "07-compressed.png");

    // ---- sanitize
    let b = e.exec(move |s| s.output_bytes(id)).unwrap();
    let clean = folio_engine::lo::sanitize(&b, SanitizeOpts { metadata: true, attachments: true, javascript: true, links: false }).unwrap();
    e.exec(move |s| s.replace_bytes(id, clean)).unwrap();
    assert_eq!(e.exec(move |s| s.info(id)).unwrap().title, "");

    // ---- encryption round trip
    let sec = Security { user_password: "open123".into(), owner_password: "own456".into(), allow_print: true, allow_copy: false, allow_modify: false, allow_annotate: true, allow_forms: true };
    e.exec(move |s| s.set_security(id, Some(sec))).unwrap();
    let enc = out.join("protected.pdf");
    let encc = enc.clone();
    e.exec(move |s| s.save_copy(id, &encc)).unwrap();
    let encc = enc.clone();
    let err = e.exec(move |s| s.open(&encc, None)).unwrap_err();
    assert_eq!(err.code, "password_required");
    let encc = enc.clone();
    let err = e.exec(move |s| s.open(&encc, Some("nope".into()))).unwrap_err();
    assert_eq!(err.code, "password_incorrect");
    let encc = enc.clone();
    let pi = e.exec(move |s| s.open(&encc, Some("open123".into()))).unwrap();
    assert_eq!(pi.page_count, 4);
    assert!(pi.protected);
    assert!(text_of(&e, pi.id, 3).contains("Secret") || text_of(&e, pi.id, 1).contains("Secret") || text_of(&e, pi.id, 2).contains("Secret"));
    // Editing a decrypted copy works (lopdf ops need plain objects).
    e.exec(move |s| s.reverse_pages(pi.id)).unwrap();
}

#[test]
fn real_world_files() {
    let e = engine();
    let tex = fixture("tex.pdf");
    let info = e.exec(move |s| s.open(&tex, None)).unwrap();
    let id = info.id;
    assert!(info.page_count >= 1);
    let t = text_of(&e, id, 0);
    assert!(t.len() > 100, "tex text: {t}");
    let objs = e.exec(move |s| s.list_objects(id, 0, false)).unwrap();
    let first_text = objs.iter().find(|o| o.kind == "text" && o.text.as_ref().map(|t| t.trim().len() > 3).unwrap_or(false)).unwrap().clone();
    // Subset font + new glyphs => replaced with standard font, still extractable.
    let idx = first_text.index;
    e.exec(move |s| s.set_object_text(id, 0, idx, "Zebra quiz 2026 ~@#".into())).unwrap();
    let tt = text_of(&e, id, 0);
    assert!(tt.contains("Zebra quiz 2026"), "edited: {:?} obj={:?} text={}", first_text, e.exec(move |s| s.list_objects(id, 0, false)).unwrap().iter().find(|o| o.index == idx), &tt[..tt.len().min(400)]);
    snapshot(&e, id, 0, "10-tex-edited.png");
    let words: Vec<&str> = t.split_whitespace().filter(|w| w.len() > 5 && w.chars().all(|c| c.is_alphabetic())).collect();
    let target = words[words.len() / 2].to_string();
    let tq = target.clone();
    let hits = e.exec(move |s| s.search(id, &tq, true, true, 5)).unwrap();
    assert!(!hits.is_empty());
    let areas: Vec<RedactArea> = hits.iter().map(|h| RedactArea { page: h.page, rect: h.rects[0] }).collect();
    let n_before = hits.len();
    let rep = e.exec(move |s| s.apply_redactions(id, areas, "#000000".into())).unwrap();
    let tq = target.clone();
    let after = e.exec(move |s| s.search(id, &tq, true, true, 50)).unwrap();
    assert!(after.len() < n_before, "{target}: {} -> {} {rep:?}", n_before, after.len());
    snapshot(&e, id, 0, "11-tex-redacted.png");
    let bmp = e.exec(move |s| s.render_bmp(id, 0, RenderOpts { scale: 1.0, annotations: true, forms: true })).unwrap();
    assert_eq!(&bmp[..2], b"BM");

    // lopdf's encrypted sample (empty user password)
    let encp = fixture("encrypted.pdf");
    let r = e.exec(move |s| s.open(&encp, None));
    match r {
        Ok(i) => assert!(i.page_count >= 1),
        Err(err) => assert_eq!(err.code, "password_required"),
    }
}

#[test]
fn bmp_channel_order() {
    let e = engine();
    let info = e.exec(|s| s.create_blank(100.0, 100.0, 1)).unwrap();
    let id = info.id;
    e.exec(move |s| s.add_annot(id, 0, NewAnnot::Shape { shape: ShapeKind::Rect, rect: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, color: "#ff0000".into(), width: 1.0, fill: Some("#ff0000".into()) }, "t")).unwrap();
    let bmp = e.exec(move |s| s.render_bmp(id, 0, RenderOpts { scale: 1.0, annotations: true, forms: true })).unwrap();
    // centre pixel, BGRA
    let (w, h) = (100usize, 100usize);
    let px = 54 + ((h / 2) * w + w / 2) * 4;
    assert_eq!(&bmp[px..px + 3], &[0, 0, 255], "expected BGR order for red");
}
