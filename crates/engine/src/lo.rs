//! Structural operations that PDFium does not expose, done with lopdf on the
//! serialized document: encryption, form-field creation, choice values, page
//! reordering, metadata, outlines, compression and sanitising.

use crate::forms::NewField;
use crate::types::OutlineItem;
use crate::{EngineError, Result, Security};
use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{dictionary, Dictionary, Document, EncryptionState, EncryptionVersion, Object, ObjectId, Permissions, Stream, StringFormat};
use std::collections::BTreeMap;
use std::sync::Arc;

fn load(bytes: &[u8]) -> Result<Document> {
    Document::load_mem(bytes).map_err(Into::into)
}

fn save(doc: &mut Document) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

fn save_compact(doc: &mut Document) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let opts = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .compression_level(9)
        .build();
    doc.save_with_options(&mut out, opts)?;
    Ok(out)
}

fn pdf_string(s: &str) -> Object {
    // UTF-16BE with BOM when needed, PDFDocEncoding-compatible ASCII otherwise.
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut v = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(v, StringFormat::Hexadecimal)
    }
}

fn decode_pdf_string(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        String::from_utf16_lossy(&u)
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

// ---------------------------------------------------------------- security

pub fn decrypt_to_plain(bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    if !doc.is_encrypted() {
        return Ok(bytes.to_vec());
    }
    if doc.encryption_state.is_none() {
        doc.decrypt(password)?;
    }
    doc.trailer.remove(b"Encrypt");
    doc.encryption_state = None;
    // Sanity check: the document must have pages after decryption.
    if doc.get_pages().is_empty() {
        return Err(EngineError::new("decrypt", "decryption produced an empty document"));
    }
    save(&mut doc)
}

pub fn encrypt(bytes: &[u8], sec: &Security) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    if doc.is_encrypted() {
        doc.trailer.remove(b"Encrypt");
        doc.encryption_state = None;
    }
    let mut perms = Permissions::empty();
    if sec.allow_print {
        perms |= Permissions::PRINTABLE | Permissions::PRINTABLE_IN_HIGH_QUALITY;
    }
    if sec.allow_copy {
        perms |= Permissions::COPYABLE | Permissions::COPYABLE_FOR_ACCESSIBILITY;
    } else {
        perms |= Permissions::COPYABLE_FOR_ACCESSIBILITY;
    }
    if sec.allow_modify {
        perms |= Permissions::MODIFIABLE | Permissions::ASSEMBLABLE;
    }
    if sec.allow_annotate {
        perms |= Permissions::ANNOTABLE;
    }
    if sec.allow_forms {
        perms |= Permissions::FILLABLE;
    }
    let owner = if sec.owner_password.is_empty() {
        // An owner password is mandatory for restrictions to mean anything;
        // derive a random one when the user only set an open password.
        let mut r = [0u8; 16];
        getrandom::fill(&mut r).map_err(|e| EngineError::new("random", e.to_string()))?;
        r.iter().map(|b| format!("{b:02x}")).collect::<String>()
    } else {
        sec.owner_password.clone()
    };
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| EngineError::new("random", e.to_string()))?;
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let version = EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: &owner,
        user_password: &sec.user_password,
        permissions: perms,
    };
    let state = EncryptionState::try_from(version)?;
    if doc.trailer.get(b"ID").is_err() {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|e| EngineError::new("random", e.to_string()))?;
        doc.trailer.set(
            "ID",
            Object::Array(vec![Object::String(id.to_vec(), StringFormat::Hexadecimal), Object::String(id.to_vec(), StringFormat::Hexadecimal)]),
        );
    }
    doc.version = "1.7".into();
    doc.encrypt(&state)?;
    save(&mut doc)
}

// ---------------------------------------------------------------- helpers

fn page_id(doc: &Document, page_index: u32) -> Result<ObjectId> {
    doc.get_pages()
        .get(&(page_index + 1))
        .copied()
        .ok_or_else(|| EngineError::invalid(format!("page {} not found", page_index + 1)))
}

/// The /Annots array of a page, resolving an indirect array.
fn page_annots(doc: &Document, pid: ObjectId) -> Vec<Object> {
    let Ok(page) = doc.get_dictionary(pid) else { return vec![] };
    match page.get(b"Annots") {
        Ok(o) => deref(doc, o).as_array().cloned().unwrap_or_default(),
        Err(_) => vec![],
    }
}

fn set_page_annots(doc: &mut Document, pid: ObjectId, annots: Vec<Object>) -> Result<()> {
    let existing_ref = doc.get_dictionary(pid)?.get(b"Annots").ok().and_then(|o| o.as_reference().ok());
    match existing_ref {
        Some(aid) => {
            *doc.get_object_mut(aid)? = Object::Array(annots);
        }
        None => {
            doc.get_dictionary_mut(pid)?.set("Annots", Object::Array(annots));
        }
    }
    Ok(())
}

fn widget_id(doc: &Document, page_index: u32, annot_index: u32) -> Result<ObjectId> {
    let pid = page_id(doc, page_index)?;
    let annots = page_annots(doc, pid);
    annots
        .get(annot_index as usize)
        .and_then(|o| o.as_reference().ok())
        .ok_or_else(|| EngineError::invalid("widget annotation not found"))
}

/// Walk up /Parent until a dictionary carrying /T (the terminal field).
fn terminal_field(doc: &Document, mut id: ObjectId) -> ObjectId {
    for _ in 0..32 {
        let Ok(d) = doc.get_dictionary(id) else { break };
        if d.has(b"T") || d.has(b"FT") && !d.has(b"Parent") {
            return id;
        }
        match d.get(b"Parent").and_then(|p| p.as_reference()) {
            Ok(p) => id = p,
            Err(_) => return id,
        }
    }
    id
}

fn inherited<'a>(doc: &'a Document, mut id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(deref(doc, v));
        }
        id = d.get(b"Parent").and_then(|p| p.as_reference()).ok()?;
    }
    None
}

fn acroform_id(doc: &mut Document) -> Result<ObjectId> {
    let cat = doc.catalog()?;
    if let Ok(r) = cat.get(b"AcroForm").and_then(|o| o.as_reference()) {
        return Ok(r);
    }
    let inline = cat.get(b"AcroForm").ok().and_then(|o| o.as_dict().ok().cloned());
    let dict = inline.unwrap_or_else(|| dictionary! { "Fields" => Object::Array(vec![]) });
    let id = doc.add_object(Object::Dictionary(dict));
    doc.catalog_mut()?.set("AcroForm", Object::Reference(id));
    Ok(id)
}

fn ensure_form_resources(doc: &mut Document, af: ObjectId) -> Result<(ObjectId, ObjectId)> {
    let existing_dr = doc.get_dictionary(af)?.get(b"DR").ok().cloned();
    let mut dr = match existing_dr {
        Some(o) => deref(doc, &o).as_dict().cloned().unwrap_or_default(),
        None => Dictionary::new(),
    };
    let mut fonts = match dr.get(b"Font") {
        Ok(o) => deref(doc, o).as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };
    let helv_ref = match fonts.get(b"Helv").and_then(|o| o.as_reference()) {
        Ok(r) => r,
        Err(_) => {
            let helv = doc.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
            });
            fonts.set("Helv", Object::Reference(helv));
            helv
        }
    };
    let zadb_ref = match fonts.get(b"ZaDb").and_then(|o| o.as_reference()) {
        Ok(r) => r,
        Err(_) => {
            let zadb = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" });
            fonts.set("ZaDb", Object::Reference(zadb));
            zadb
        }
    };
    dr.set("Font", Object::Dictionary(fonts));
    let a = doc.get_dictionary_mut(af)?;
    a.set("DR", Object::Dictionary(dr));
    if !a.has(b"DA") {
        a.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    }
    a.set("NeedAppearances", Object::Boolean(true));
    a.remove(b"XFA");
    if !a.has(b"Fields") {
        a.set("Fields", Object::Array(vec![]));
    }
    Ok((helv_ref, zadb_ref))
}

/// WinAnsi-escaped PDF literal for content streams.
fn content_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let b: u8 = match c {
            '\\' => { out.push_str("\\\\"); continue; }
            '(' => { out.push_str("\\("); continue; }
            ')' => { out.push_str("\\)"); continue; }
            c if (c as u32) < 0x20 => b' ',
            c if (c as u32) < 0x7f => c as u8,
            c if (0xa0..=0xff).contains(&(c as u32)) => c as u32 as u8,
            '€' => 0x80, '‘' => 0x91, '’' => 0x92, '“' => 0x93, '”' => 0x94, '–' => 0x96, '—' => 0x97, '•' => 0x95,
            _ => b'?',
        };
        if b >= 0x80 {
            out.push_str(&format!("\\{:03o}", b));
        } else {
            out.push(b as char);
        }
    }
    out.push(')');
    out
}

/// Build and attach a normal appearance for a text or choice widget so the
/// value is visible in every viewer and survives flattening.
fn build_value_ap(doc: &mut Document, wid: ObjectId, value: &str) -> Result<()> {
    let af = acroform_id(doc)?;
    let (helv, _) = ensure_form_resources(doc, af)?;
    let d = doc.get_dictionary(wid)?.clone();
    let rect: Vec<f32> = d.get(b"Rect").ok().and_then(|r| deref(doc, r).as_array().ok().cloned()).unwrap_or_default().iter().filter_map(|o| o.as_float().ok()).collect();
    if rect.len() != 4 {
        return Ok(());
    }
    let mk = d.get(b"MK").ok().and_then(|m| deref(doc, m).as_dict().ok().cloned()).unwrap_or_default();
    let rot = mk.get(b"R").and_then(|r| r.as_i64()).unwrap_or(0).rem_euclid(360);
    let (rw, rh) = ((rect[2] - rect[0]).abs(), (rect[3] - rect[1]).abs());
    let (w, h) = if rot % 180 == 90 { (rh, rw) } else { (rw, rh) };
    let ff = inherited(doc, wid, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    let ft = inherited(doc, wid, b"FT").and_then(|o| o.as_name().ok()).map(|n| n.to_vec()).unwrap_or_default();
    let multiline = ft == b"Tx" && ff & 4096 != 0;
    let da = inherited(doc, wid, b"DA").and_then(|o| o.as_str().ok()).map(decode_pdf_string).unwrap_or_default();
    let mut fs: f32 = da.split_whitespace().collect::<Vec<_>>().windows(2).find(|p| p[1] == "Tf").and_then(|p| p[0].parse().ok()).unwrap_or(0.0);
    let color_ops = {
        let parts: Vec<&str> = da.split_whitespace().collect();
        let mut c = String::from("0 g");
        for (i, p) in parts.iter().enumerate() {
            if (*p == "g" && i >= 1) || (*p == "rg" && i >= 3) {
                let n = if *p == "g" { 1 } else { 3 };
                c = format!("{} {}", parts[i - n..i].join(" "), p);
            }
        }
        c
    };
    let mut ops = String::new();
    if let Ok(bg) = mk.get(b"BG").and_then(|o| o.as_array()) {
        let v: Vec<String> = bg.iter().filter_map(|o| o.as_float().ok()).map(|f| format!("{f:.3}")).collect();
        if v.len() == 3 {
            ops.push_str(&format!("q {} rg 0 0 {w:.2} {h:.2} re f Q\n", v.join(" ")));
        }
    }
    if let Ok(bc) = mk.get(b"BC").and_then(|o| o.as_array()) {
        let v: Vec<String> = bc.iter().filter_map(|o| o.as_float().ok()).map(|f| format!("{f:.3}")).collect();
        if v.len() == 3 {
            ops.push_str(&format!("q {} RG 1 w 0.5 0.5 {:.2} {:.2} re S Q\n", v.join(" "), (w - 1.0).max(0.0), (h - 1.0).max(0.0)));
        }
    }
    ops.push_str("/Tx BMC\nq 1 1 ");
    ops.push_str(&format!("{:.2} {:.2} re W n\n", (w - 2.0).max(0.0), (h - 2.0).max(0.0)));
    let display = if ft == b"Ch" {
        // Show the display text for an export value.
        let mut disp = value.to_string();
        if let Some(opt) = inherited(doc, wid, b"Opt").and_then(|o| o.as_array().ok()) {
            for o in opt {
                if let Ok(pair) = deref(doc, o).as_array() {
                    if pair.len() == 2 && pair[0].as_str().map(decode_pdf_string).unwrap_or_default() == value {
                        disp = pair[1].as_str().map(decode_pdf_string).unwrap_or_default();
                    }
                }
            }
        }
        disp
    } else {
        value.to_string()
    };
    if multiline {
        if fs <= 0.0 {
            fs = 10.0;
        }
        let lines = crate::annots::wrap_lines(&display, w - 4.0, fs, crate::stamps::FontFamily::Helvetica, false);
        ops.push_str(&format!("BT /Helv {fs:.2} Tf {color_ops} {:.2} TL 2 {:.2} Td\n", fs * 1.15, h - 2.0 - fs));
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                ops.push_str("T* ");
            }
            ops.push_str(&format!("{} Tj\n", content_string(l)));
        }
        ops.push_str("ET\n");
    } else {
        if fs <= 0.0 {
            fs = (h * 0.62).clamp(4.0, 12.0);
            let tw = crate::stamps::approx_width(&display, fs, crate::stamps::FontFamily::Helvetica, false);
            if tw > w - 4.0 && tw > 0.0 {
                fs = (fs * (w - 4.0) / tw).max(4.0);
            }
        }
        let q = inherited(doc, wid, b"Q").and_then(|o| o.as_i64().ok()).unwrap_or(0);
        let tw = crate::stamps::approx_width(&display, fs, crate::stamps::FontFamily::Helvetica, false);
        let x = match q {
            1 => (w - tw) / 2.0,
            2 => w - 2.0 - tw,
            _ => 2.0,
        };
        let y = (h - fs * 0.78) / 2.0;
        ops.push_str(&format!("BT /Helv {fs:.2} Tf {color_ops} {x:.2} {y:.2} Td {} Tj ET\n", content_string(&display)));
    }
    ops.push_str("Q\nEMC\n");
    let (a, b, c, dd) = match rot {
        90 => (0.0, 1.0, -1.0, 0.0),
        180 => (-1.0, 0.0, 0.0, -1.0),
        270 => (0.0, -1.0, 1.0, 0.0),
        _ => (1.0, 0.0, 0.0, 1.0),
    };
    let stream = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
            "Matrix" => vec![Object::Real(a), Object::Real(b), Object::Real(c), Object::Real(dd), 0.into(), 0.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "Helv" => Object::Reference(helv) } },
        },
        ops.into_bytes(),
    );
    let sid = doc.add_object(stream);
    doc.get_dictionary_mut(wid)?.set("AP", dictionary! { "N" => Object::Reference(sid) });
    Ok(())
}

fn set_need_appearances(doc: &mut Document) -> Result<()> {
    let af = acroform_id(doc)?;
    doc.get_dictionary_mut(af)?.set("NeedAppearances", Object::Boolean(true));
    Ok(())
}

fn push_field(doc: &mut Document, af: ObjectId, field: ObjectId) -> Result<()> {
    let fields_ref = doc.get_dictionary(af)?.get(b"Fields").ok().and_then(|o| o.as_reference().ok());
    match fields_ref {
        Some(fid) => doc.get_object_mut(fid)?.as_array_mut()?.push(Object::Reference(field)),
        None => doc.get_dictionary_mut(af)?.get_mut(b"Fields")?.as_array_mut()?.push(Object::Reference(field)),
    }
    Ok(())
}

fn ap_stream(w: f32, h: f32, content: String, resources: Dictionary) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
            "Resources" => resources,
        },
        content.into_bytes(),
    )
}

// ---------------------------------------------------------------- forms

pub fn add_field(bytes: &[u8], spec: &NewField, rect: [f32; 4], rotation: u32) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let pid = page_id(&doc, spec.page)?;
    let af = acroform_id(&mut doc)?;
    let (_helv, zadb) = ensure_form_resources(&mut doc, af)?;
    let [l, b, r, t] = rect;
    // Widget-local width/height as seen upright (MK /R handles rotation).
    let (w, h) = if rotation % 180 == 90 { (t - b, r - l) } else { (r - l, t - b) };
    let fs = if spec.font_size > 0.0 { spec.font_size } else { 0.0 };
    let mut flags: i64 = if spec.required { 2 } else { 0 };
    let rect_obj = Object::Array(vec![Object::Real(l), Object::Real(b), Object::Real(r), Object::Real(t)]);
    let mk = dictionary! {
        "BC" => vec![Object::Real(0.25), Object::Real(0.25), Object::Real(0.25)],
        "BG" => vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
        "R" => rotation as i64,
    };
    let border_ops = format!("q 0.25 0.25 0.25 RG 1 w 0.5 0.5 {:.2} {:.2} re S Q\n", (w - 1.0).max(0.0), (h - 1.0).max(0.0));
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(EngineError::invalid("field name is required"));
    }

    let mut widget = dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect_obj, "F" => 4,
        "P" => Object::Reference(pid), "MK" => mk,
        "DA" => Object::string_literal(format!("/Helv {fs} Tf 0 g")),
    };
    let mut new_terminal: Option<ObjectId> = None;

    match spec.kind.as_str() {
        "text" | "multiline" => {
            if spec.kind == "multiline" {
                flags |= 4096;
            }
            widget.set("FT", "Tx");
            widget.set("T", pdf_string(name));
            widget.set("Ff", flags);
            widget.set("V", pdf_string(""));
            let ap = doc.add_object(ap_stream(w, h, border_ops.clone(), Dictionary::new()));
            widget.set("AP", dictionary! { "N" => Object::Reference(ap) });
        }
        "checkbox" | "radio" => {
            let is_radio = spec.kind == "radio";
            let on_name = if is_radio {
                let v = if spec.export_value.trim().is_empty() { "Choice1" } else { spec.export_value.trim() };
                v.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            } else {
                "Yes".to_string()
            };
            let glyph = if is_radio { "l" } else { "4" };
            let gs = (h.min(w) * 0.78).max(4.0);
            let res = dictionary! { "Font" => dictionary! { "ZaDb" => Object::Reference(zadb) } };
            let on_ops = format!(
                "{border_ops}q BT 0 g /ZaDb {gs:.2} Tf {:.2} {:.2} Td ({glyph}) Tj ET Q\n",
                (w - gs * 0.75) / 2.0,
                (h - gs * 0.7) / 2.0
            );
            let on = doc.add_object(ap_stream(w, h, on_ops, res.clone()));
            let off = doc.add_object(ap_stream(w, h, border_ops.clone(), Dictionary::new()));
            let mut n = Dictionary::new();
            n.set(on_name.as_bytes().to_vec(), Object::Reference(on));
            n.set("Off", Object::Reference(off));
            widget.set("AP", dictionary! { "N" => n });
            widget.set("AS", "Off");
            widget.set("DA", Object::string_literal("/ZaDb 0 Tf 0 g"));
            if let Ok(mk) = widget.get_mut(b"MK").and_then(|m| m.as_dict_mut()) {
                mk.set("CA", Object::string_literal(glyph));
            }
            if is_radio {
                // Find an existing radio group with this name, or create it.
                let group = find_field_by_name(&doc, af, name);
                let gid = match group {
                    Some(g) => g,
                    None => {
                        let g = doc.add_object(dictionary! {
                            "FT" => "Btn", "Ff" => (flags | 49152), "T" => pdf_string(name),
                            "V" => "Off", "Kids" => Object::Array(vec![]),
                        });
                        push_field(&mut doc, af, g)?;
                        g
                    }
                };
                widget.set("Parent", Object::Reference(gid));
                let wid = doc.add_object(Object::Dictionary(widget));
                doc.get_dictionary_mut(gid)?.get_mut(b"Kids")?.as_array_mut()?.push(Object::Reference(wid));
                let mut annots = page_annots(&doc, pid);
                annots.push(Object::Reference(wid));
                set_page_annots(&mut doc, pid, annots)?;
                return save(&mut doc);
            }
            widget.set("FT", "Btn");
            widget.set("T", pdf_string(name));
            widget.set("Ff", flags);
            widget.set("V", "Off");
        }
        "combo" | "list" => {
            if spec.kind == "combo" {
                flags |= 131072;
            }
            widget.set("FT", "Ch");
            widget.set("T", pdf_string(name));
            widget.set("Ff", flags);
            widget.set("Opt", Object::Array(spec.options.iter().map(|o| pdf_string(o)).collect()));
            if let Some(first) = spec.options.first() {
                if spec.kind == "combo" {
                    widget.set("V", pdf_string(first));
                }
            }
        }
        "signature" => {
            widget.set("FT", "Sig");
            widget.set("T", pdf_string(name));
            let ap = doc.add_object(ap_stream(w, h, border_ops.clone(), Dictionary::new()));
            widget.set("AP", dictionary! { "N" => Object::Reference(ap) });
        }
        other => return Err(EngineError::invalid(format!("unknown field type {other}"))),
    }
    if find_field_by_name(&doc, af, name).is_some() {
        return Err(EngineError::invalid(format!("a field named \"{name}\" already exists")));
    }
    let wid = doc.add_object(Object::Dictionary(widget));
    new_terminal.get_or_insert(wid);
    push_field(&mut doc, af, wid)?;
    let mut annots = page_annots(&doc, pid);
    annots.push(Object::Reference(wid));
    set_page_annots(&mut doc, pid, annots)?;
    save(&mut doc)
}

fn find_field_by_name(doc: &Document, af: ObjectId, name: &str) -> Option<ObjectId> {
    let fields = doc.get_dictionary(af).ok()?.get(b"Fields").ok()?;
    let fields = deref(doc, fields).as_array().ok()?.clone();
    for f in fields {
        let Ok(fid) = f.as_reference() else { continue };
        if let Ok(d) = doc.get_dictionary(fid) {
            if let Ok(t) = d.get(b"T").and_then(|t| t.as_str()) {
                if decode_pdf_string(t) == name {
                    return Some(fid);
                }
            }
        }
    }
    None
}

pub fn set_choice_value(bytes: &[u8], page_index: u32, annot_index: u32, value: &str) -> Result<Vec<u8>> {
    set_field_value(bytes, page_index, annot_index, &crate::forms::FieldValue::Choice { value: value.to_string() })
}

pub fn delete_widget(bytes: &[u8], page_index: u32, annot_index: u32) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let pid = page_id(&doc, page_index)?;
    let wid = widget_id(&doc, page_index, annot_index)?;
    let mut annots = page_annots(&doc, pid);
    annots.retain(|o| o.as_reference().ok() != Some(wid));
    set_page_annots(&mut doc, pid, annots)?;
    let parent = doc.get_dictionary(wid)?.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    if let Some(p) = parent {
        if let Ok(kids) = doc.get_dictionary_mut(p).and_then(|d| d.get_mut(b"Kids")).and_then(|k| k.as_array_mut()) {
            kids.retain(|o| o.as_reference().ok() != Some(wid));
        }
    }
    let af = acroform_id(&mut doc)?;
    let fields_ref = doc.get_dictionary(af)?.get(b"Fields").ok().and_then(|o| o.as_reference().ok());
    let fields = match fields_ref {
        Some(r) => doc.get_object_mut(r)?.as_array_mut()?,
        None => doc.get_dictionary_mut(af)?.get_mut(b"Fields")?.as_array_mut()?,
    };
    fields.retain(|o| o.as_reference().ok() != Some(wid));
    doc.delete_object(wid);
    save(&mut doc)
}

pub fn drop_acroform(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    doc.catalog_mut()?.remove(b"AcroForm");
    save(&mut doc)
}

pub fn reset_form(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Ok(d) = doc.get_dictionary(id) else { continue };
        let is_field = d.has(b"FT") || d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| n == b"Widget").unwrap_or(false);
        if !is_field {
            continue;
        }
        let ft = inherited(&doc, id, b"FT").and_then(|o| o.as_name().ok()).map(|n| n.to_vec());
        let dv = d.get(b"DV").ok().cloned();
        let d = doc.get_dictionary_mut(id)?;
        match ft.as_deref() {
            Some(b"Btn") => {
                if d.has(b"V") {
                    d.set("V", dv.unwrap_or(Object::Name(b"Off".to_vec())));
                }
                if d.has(b"AS") {
                    d.set("AS", "Off");
                }
            }
            Some(b"Tx") | Some(b"Ch") => {
                match dv {
                    Some(v) => d.set("V", v),
                    None => {
                        d.remove(b"V");
                    }
                }
                if d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| n == b"Widget").unwrap_or(false) {
                    d.remove(b"AP");
                }
            }
            _ => {}
        }
    }
    set_need_appearances(&mut doc)?;
    save(&mut doc)
}

// ---------------------------------------------------------------- pages

/// Reorder pages. `order[i]` is the current index of the page that should end
/// up at position i. Inheritable attributes are pushed down to each page
/// before the tree is flattened, so nothing is lost.
pub fn reorder_pages(bytes: &[u8], order: &[u32]) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    if order.len() != pages.len() {
        return Err(EngineError::invalid("order must list every page exactly once"));
    }
    let mut seen = vec![false; pages.len()];
    for &o in order {
        let o = o as usize;
        if o >= pages.len() || seen[o] {
            return Err(EngineError::invalid("order must list every page exactly once"));
        }
        seen[o] = true;
    }
    for &pid in &pages {
        for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
            let has = doc.get_dictionary(pid)?.has(key);
            if !has {
                let parent = doc.get_dictionary(pid)?.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
                if let Some(v) = parent.and_then(|p| inherited(&doc, p, key)).cloned() {
                    doc.get_dictionary_mut(pid)?.set(key.to_vec(), v);
                }
            }
        }
    }
    let root_pages = doc.catalog()?.get(b"Pages")?.as_reference()?;
    let kids: Vec<Object> = order.iter().map(|&i| Object::Reference(pages[i as usize])).collect();
    for &pid in &pages {
        doc.get_dictionary_mut(pid)?.set("Parent", Object::Reference(root_pages));
    }
    {
        let rp = doc.get_dictionary_mut(root_pages)?;
        rp.set("Kids", Object::Array(kids));
        rp.set("Count", pages.len() as i64);
        rp.remove(b"Parent");
    }
    doc.prune_objects();
    save(&mut doc)
}

// ---------------------------------------------------------------- metadata & outline

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct MetadataUpdate {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
}

pub fn set_metadata(bytes: &[u8], m: &MetadataUpdate) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let info_id = match doc.trailer.get(b"Info").and_then(|o| o.as_reference()) {
        Ok(id) => id,
        Err(_) => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    let now = pdf_date_now();
    let info = doc.get_dictionary_mut(info_id)?;
    for (k, v) in [("Title", &m.title), ("Author", &m.author), ("Subject", &m.subject), ("Keywords", &m.keywords), ("Creator", &m.creator)] {
        if let Some(v) = v {
            if v.is_empty() {
                info.remove(k.as_bytes());
            } else {
                info.set(k, pdf_string(v));
            }
        }
    }
    info.set("ModDate", Object::string_literal(now));
    info.set("Producer", Object::string_literal("Folio PDF"));
    // XMP would now disagree with /Info; drop it so readers use /Info.
    doc.catalog_mut()?.remove(b"Metadata");
    save(&mut doc)
}

fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Civil-from-days (UTC).
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("D:{y:04}{m:02}{d:02}{h:02}{mi:02}{s:02}Z")
}

/// Replace the whole outline (bookmarks) tree.
pub fn set_outline(bytes: &[u8], items: &[OutlineItem]) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    doc.catalog_mut()?.remove(b"Outlines");
    if items.is_empty() {
        doc.prune_objects();
        return save(&mut doc);
    }
    let root = doc.add_object(dictionary! { "Type" => "Outlines" });

    fn build(doc: &mut Document, pages: &[ObjectId], parent: ObjectId, items: &[OutlineItem]) -> (Option<ObjectId>, Option<ObjectId>, i64) {
        let ids: Vec<ObjectId> = items.iter().map(|_| doc.new_object_id()).collect();
        let mut total = 0i64;
        for (i, it) in items.iter().enumerate() {
            let mut d = dictionary! { "Title" => pdf_string(&it.title), "Parent" => Object::Reference(parent) };
            if i > 0 {
                d.set("Prev", Object::Reference(ids[i - 1]));
            }
            if i + 1 < ids.len() {
                d.set("Next", Object::Reference(ids[i + 1]));
            }
            if let Some(p) = it.page.and_then(|p| pages.get(p as usize)) {
                d.set("Dest", Object::Array(vec![Object::Reference(*p), "XYZ".into(), Object::Null, Object::Null, Object::Null]));
            }
            let (first, last, count) = build(doc, pages, ids[i], &it.children);
            if let (Some(f), Some(l)) = (first, last) {
                d.set("First", Object::Reference(f));
                d.set("Last", Object::Reference(l));
                d.set("Count", count);
            }
            doc.objects.insert(ids[i], Object::Dictionary(d));
            total += 1 + count;
        }
        (ids.first().copied(), ids.last().copied(), total)
    }

    let (first, last, count) = build(&mut doc, &pages, root, items);
    {
        let r = doc.get_dictionary_mut(root)?;
        if let (Some(f), Some(l)) = (first, last) {
            r.set("First", Object::Reference(f));
            r.set("Last", Object::Reference(l));
        }
        r.set("Count", count);
    }
    doc.catalog_mut()?.set("Outlines", Object::Reference(root));
    doc.prune_objects();
    save(&mut doc)
}

// ---------------------------------------------------------------- optimise

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CompressReport {
    pub before: usize,
    pub after: usize,
    pub images_recompressed: u32,
}

pub fn compress(bytes: &[u8], level: CompressLevel) -> Result<(Vec<u8>, CompressReport)> {
    let mut doc = load(bytes)?;
    let (max_px, quality) = match level {
        CompressLevel::Low => (2600u32, 85u8),
        CompressLevel::Medium => (1800, 72),
        CompressLevel::High => (1200, 55),
    };
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    let mut recompressed = 0;
    for id in ids {
        let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else { continue };
        let d = &stream.dict;
        if d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| n != b"Image").unwrap_or(true) {
            continue;
        }
        if d.get(b"ImageMask").and_then(|o| o.as_bool()).unwrap_or(false) || d.has(b"Decode") || d.has(b"Mask") && d.get(b"Mask").map(|m| m.as_array().is_ok()).unwrap_or(false) {
            continue;
        }
        let w = d.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let h = d.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let bpc = d.get(b"BitsPerComponent").and_then(|o| o.as_i64()).unwrap_or(8);
        if w < 16 || h < 16 || bpc != 8 {
            continue;
        }
        let comps = match d.get(b"ColorSpace").map(|cs| deref(&doc, cs)) {
            Ok(Object::Name(n)) if n == b"DeviceRGB" || n == b"CalRGB" => 3,
            Ok(Object::Name(n)) if n == b"DeviceGray" || n == b"CalGray" => 1,
            Ok(Object::Array(a)) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => {
                let n = a
                    .get(1)
                    .and_then(|r| doc.dereference(r).ok())
                    .and_then(|(_, o)| o.as_stream().ok())
                    .and_then(|s| s.dict.get(b"N").and_then(|n| n.as_i64()).ok())
                    .unwrap_or(0);
                if n == 3 || n == 1 { n as u8 } else { continue }
            }
            _ => continue,
        };
        let filters: Vec<Vec<u8>> = match d.get(b"Filter") {
            Ok(Object::Name(n)) => vec![n.clone()],
            Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok().map(|n| n.to_vec())).collect(),
            _ => vec![],
        };
        let decoded: Option<image::DynamicImage> = if filters == [b"DCTDecode".to_vec()] {
            image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok()
        } else if filters.iter().all(|f| f == b"FlateDecode") {
            let raw = if filters.is_empty() { Ok(stream.content.clone()) } else { stream.decompressed_content() };
            match raw {
                Ok(raw) if raw.len() >= (w * h * comps as u32) as usize => {
                    let raw = raw[..(w * h * comps as u32) as usize].to_vec();
                    if comps == 3 {
                        image::RgbImage::from_raw(w, h, raw).map(image::DynamicImage::ImageRgb8)
                    } else {
                        image::GrayImage::from_raw(w, h, raw).map(image::DynamicImage::ImageLuma8)
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        let Some(mut img) = decoded else { continue };
        if img.width().max(img.height()) > max_px {
            img = img.resize(max_px, max_px, image::imageops::FilterType::Triangle);
        }
        let mut jpeg = Vec::new();
        let enc_ok = if comps == 1 {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode_image(&img.to_luma8()).is_ok()
        } else {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode_image(&img.to_rgb8()).is_ok()
        };
        if !enc_ok || jpeg.len() as f32 >= stream.content.len() as f32 * 0.9 {
            continue;
        }
        let (nw, nh) = (img.width(), img.height());
        let s = doc.get_object_mut(id)?.as_stream_mut()?;
        s.dict.set("Filter", "DCTDecode");
        s.dict.remove(b"DecodeParms");
        s.dict.set("Width", nw as i64);
        s.dict.set("Height", nh as i64);
        s.dict.set("BitsPerComponent", 8);
        s.set_content(jpeg);
        s.allows_compression = false;
        recompressed += 1;
    }
    doc.compress();
    doc.prune_objects();
    let out = save_compact(&mut doc)?;
    let report = CompressReport { before: bytes.len(), after: out.len(), images_recompressed: recompressed };
    if out.len() >= bytes.len() {
        return Ok((bytes.to_vec(), CompressReport { after: bytes.len(), ..report }));
    }
    Ok((out, report))
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct SanitizeOpts {
    pub metadata: bool,
    pub attachments: bool,
    pub javascript: bool,
    pub links: bool,
}

pub fn sanitize(bytes: &[u8], o: SanitizeOpts) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    if o.metadata {
        doc.trailer.remove(b"Info");
        let cat = doc.catalog_mut()?;
        cat.remove(b"Metadata");
        cat.remove(b"PieceInfo");
    }
    let names_ref = doc.catalog()?.get(b"Names").ok().cloned();
    if let Some(n) = names_ref {
        let nid = n.as_reference().ok();
        let mut names = deref(&doc, &n).as_dict().cloned().unwrap_or_default();
        if o.attachments {
            names.remove(b"EmbeddedFiles");
        }
        if o.javascript {
            names.remove(b"JavaScript");
        }
        match nid {
            Some(id) => *doc.get_object_mut(id)? = Object::Dictionary(names),
            None => doc.catalog_mut()?.set("Names", Object::Dictionary(names)),
        }
    }
    if o.javascript {
        let cat = doc.catalog_mut()?;
        cat.remove(b"AA");
        cat.remove(b"OpenAction");
        let af = cat.get(b"AcroForm").ok().cloned();
        if let Some(af) = af {
            if let Ok(id) = af.as_reference() {
                if let Ok(d) = doc.get_dictionary_mut(id) {
                    d.remove(b"XFA");
                }
            }
        }
    }
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for pid in pages {
        if o.javascript || o.metadata {
            if let Ok(p) = doc.get_dictionary_mut(pid) {
                if o.javascript {
                    p.remove(b"AA");
                }
                if o.metadata {
                    p.remove(b"PieceInfo");
                    p.remove(b"Metadata");
                }
            }
        }
        if o.attachments || o.links || o.javascript {
            let annots = page_annots(&doc, pid);
            let keep: Vec<Object> = annots
                .into_iter()
                .filter(|a| {
                    let Ok(d) = deref(&doc, a).as_dict() else { return true };
                    let st = d.get(b"Subtype").and_then(|s| s.as_name()).unwrap_or(b"");
                    !(o.attachments && st == b"FileAttachment" || o.links && st == b"Link")
                })
                .collect();
            set_page_annots(&mut doc, pid, keep)?;
        }
    }
    if o.javascript {
        // Strip JavaScript actions anywhere in the file.
        let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
        for id in ids {
            if let Ok(d) = doc.get_dictionary_mut(id) {
                if d.get(b"S").and_then(|s| s.as_name()).map(|n| n == b"JavaScript").unwrap_or(false) {
                    d.remove(b"JS");
                    d.set("S", "Named");
                    d.set("N", "Nothing");
                }
                d.remove(b"AA");
            }
        }
    }
    doc.prune_objects();
    save(&mut doc)
}

/// Set an annotation's /C colour and drop its appearance so viewers rebuild it.
pub fn set_annot_color(bytes: &[u8], page_index: u32, annot_index: u32, hex: &str) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let aid = widget_id(&doc, page_index, annot_index)?;
    let c = crate::types::parse_color(hex);
    let arr = Object::Array(vec![
        Object::Real(c.red() as f32 / 255.0),
        Object::Real(c.green() as f32 / 255.0),
        Object::Real(c.blue() as f32 / 255.0),
    ]);
    let d = doc.get_dictionary_mut(aid)?;
    d.set("C", arr);
    d.remove(b"AP");
    save(&mut doc)
}

/// Set a field value at the dictionary level. Works for split field/widget
/// structures, clears stale appearances for text/choice fields and switches
/// /AS on checkbox and radio widgets.
pub fn set_field_value(bytes: &[u8], page_index: u32, annot_index: u32, value: &crate::forms::FieldValue) -> Result<Vec<u8>> {
    use crate::forms::FieldValue;
    let mut doc = load(bytes)?;
    let wid = widget_id(&doc, page_index, annot_index)?;
    let fid = terminal_field(&doc, wid);
    let ft = inherited(&doc, fid, b"FT").and_then(|o| o.as_name().ok()).map(|n| n.to_vec()).unwrap_or_default();
    let kids: Vec<ObjectId> = doc
        .get_dictionary(fid)?
        .get(b"Kids")
        .ok()
        .and_then(|k| deref(&doc, k).as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let widgets: Vec<ObjectId> = if kids.is_empty() { vec![fid] } else { kids };
    let on_state = |doc: &Document, w: ObjectId| -> Option<Vec<u8>> {
        let d = doc.get_dictionary(w).ok()?;
        let ap = deref(doc, d.get(b"AP").ok()?).as_dict().ok()?;
        let n = deref(doc, ap.get(b"N").ok()?).as_dict().ok()?;
        n.iter().map(|(k, _)| k.clone()).find(|k| k.as_slice() != b"Off")
    };
    match (ft.as_slice(), value) {
        (b"Btn", FieldValue::Check { checked }) => {
            let target_on = on_state(&doc, wid).unwrap_or_else(|| b"Yes".to_vec());
            let v = if *checked { target_on.clone() } else { b"Off".to_vec() };
            doc.get_dictionary_mut(fid)?.set("V", Object::Name(v.clone()));
            for w in widgets {
                let st = on_state(&doc, w);
                let as_ = if *checked && st.as_deref() == Some(target_on.as_slice()) { target_on.clone() } else { b"Off".to_vec() };
                if let Ok(d) = doc.get_dictionary_mut(w) {
                    d.set("AS", Object::Name(as_));
                }
            }
        }
        (b"Tx", FieldValue::Text { value }) | (b"Ch", FieldValue::Text { value }) | (b"Ch", FieldValue::Choice { value }) | (b"Tx", FieldValue::Choice { value }) => {
            drop(widgets);
            return set_text_or_choice(doc, wid, fid, value);
        }
        (t, v) => return Err(EngineError::invalid(format!("cannot set {v:?} on field type {}", String::from_utf8_lossy(t)))),
    }
    save(&mut doc)
}

fn set_text_or_choice(mut doc: Document, wid: ObjectId, fid: ObjectId, value: &str) -> Result<Vec<u8>> {
    let mut export = value.to_string();
    if let Some(opt) = inherited(&doc, fid, b"Opt").and_then(|o| o.as_array().ok()) {
        for o in opt {
            if let Ok(pair) = deref(&doc, o).as_array() {
                if pair.len() == 2 {
                    let ex = pair[0].as_str().map(decode_pdf_string).unwrap_or_default();
                    let disp = pair[1].as_str().map(decode_pdf_string).unwrap_or_default();
                    if disp == value {
                        export = ex;
                    }
                }
            }
        }
    }
    {
        let f = doc.get_dictionary_mut(fid)?;
        f.set("V", pdf_string(&export));
        f.remove(b"I");
        f.remove(b"RV");
    }
    let kids: Vec<ObjectId> = doc
        .get_dictionary(fid)?
        .get(b"Kids")
        .ok()
        .and_then(|k| k.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let mut targets: Vec<ObjectId> = kids;
    targets.push(wid);
    targets.push(fid);
    targets.sort();
    targets.dedup();
    for w in targets {
        let is_widget = doc
            .get_dictionary(w)
            .map(|d| d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| n == b"Widget").unwrap_or(false))
            .unwrap_or(false);
        if is_widget {
            build_value_ap(&mut doc, w, &export)?;
        }
    }
    set_need_appearances(&mut doc)?;
    save(&mut doc)
}
