use crate::geom::Mapper;
use crate::lo;
use crate::types::{FieldInfo, Rect};
use crate::{EngineError, Result, State};
use pdfium_render::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldValue {
    Text { value: String },
    Check { checked: bool },
    Choice { value: String },
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewField {
    pub page: u32,
    /// text | multiline | checkbox | radio | combo | list | signature
    pub kind: String,
    pub name: String,
    pub rect: Rect,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub font_size: f32,
    /// Export value for radio buttons; radios sharing `name` form a group.
    #[serde(default)]
    pub export_value: String,
}

fn field_kind(t: PdfFormFieldType) -> &'static str {
    match t {
        PdfFormFieldType::PushButton => "button",
        PdfFormFieldType::Checkbox => "checkbox",
        PdfFormFieldType::RadioButton => "radio",
        PdfFormFieldType::ComboBox => "combo",
        PdfFormFieldType::ListBox => "list",
        PdfFormFieldType::Text => "text",
        PdfFormFieldType::Signature => "signature",
        _ => "unknown",
    }
}

impl State {
    pub fn list_fields(&self, id: u32, index: u32) -> Result<Vec<FieldInfo>> {
        let page = self.page(id, index)?;
        let map = Mapper::new(&page);
        let mut out = Vec::new();
        for (i, a) in page.annotations().iter().enumerate() {
            let Some(f) = a.as_form_field() else { continue };
            let rect = a.bounds().map(|b| map.rect_to_view(&b)).unwrap_or_default();
            let kind = field_kind(f.field_type());
            let (mut value, mut checked, mut options) = (String::new(), false, Vec::new());
            match f.field_type() {
                PdfFormFieldType::Text => {
                    value = f.as_text_field().and_then(|t| t.value()).unwrap_or_default();
                }
                PdfFormFieldType::Checkbox => {
                    checked = f.as_checkbox_field().map(|c| c.is_checked().unwrap_or(false)).unwrap_or(false);
                }
                PdfFormFieldType::RadioButton => {
                    if let Some(r) = f.as_radio_button_field() {
                        checked = r.is_checked().unwrap_or(false);
                        value = r.appearance_mode_value(PdfAppearanceMode::Normal).unwrap_or_default();
                    }
                }
                PdfFormFieldType::ComboBox => {
                    if let Some(c) = f.as_combo_box_field() {
                        value = c.value().unwrap_or_default();
                        options = c.options().iter().filter_map(|o| o.label().cloned()).collect();
                    }
                }
                PdfFormFieldType::ListBox => {
                    if let Some(c) = f.as_list_box_field() {
                        value = c.value().unwrap_or_default();
                        options = c.options().iter().filter_map(|o| o.label().cloned()).collect();
                    }
                }
                _ => {}
            }
            out.push(FieldInfo {
                page: index,
                annot_index: i as u32,
                name: f.name().unwrap_or_default(),
                kind: kind.into(),
                rect,
                value,
                checked,
                options,
                read_only: f.is_read_only(),
                required: f.is_required(),
            });
        }
        Ok(out)
    }

    pub fn set_field(&mut self, id: u32, index: u32, annot: u32, value: FieldValue) -> Result<()> {
        {
            let page = self.page(id, index)?;
            let a = page.annotations().get(annot as PdfPageAnnotationIndex)?;
            let f = a.as_form_field().ok_or_else(|| EngineError::invalid("not a form field"))?;
            if f.is_read_only() {
                return Err(EngineError::invalid("this field is read-only"));
            }
        }
        self.checkpoint(id)?;
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::set_field_value(&bytes, index, annot, &value)?;
        self.replace_bytes(id, out)
    }

    pub fn add_field(&mut self, id: u32, spec: NewField) -> Result<()> {
        self.checkpoint(id)?;
        let page = self.page(id, spec.page)?;
        let map = Mapper::new(&page);
        let pr = map.rect_to_page(&spec.rect);
        let rotation = map.rotation();
        drop(page);
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::add_field(
            &bytes,
            &spec,
            [pr.left().value, pr.bottom().value, pr.right().value, pr.top().value],
            rotation,
        )?;
        self.replace_bytes(id, out)
    }

    pub fn delete_field(&mut self, id: u32, index: u32, annot: u32) -> Result<()> {
        self.checkpoint(id)?;
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::delete_widget(&bytes, index, annot)?;
        self.replace_bytes(id, out)
    }

    /// Bake annotations and form fields into page content.
    pub fn flatten(&mut self, id: u32) -> Result<()> {
        self.checkpoint(id)?;
        let n = self.doc(id)?.pdf.pages().len();
        for i in 0..n {
            let mut page = self.page(id, i as u32)?;
            page.flatten()?;
        }
        // Remove the now-empty AcroForm.
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::drop_acroform(&bytes)?;
        self.replace_bytes(id, out)
    }

    pub fn reset_form(&mut self, id: u32) -> Result<()> {
        self.checkpoint(id)?;
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = lo::reset_form(&bytes)?;
        self.replace_bytes(id, out)
    }

    /// Export all field values as JSON { name: value }.
    pub fn export_form_data(&self, id: u32) -> Result<serde_json::Value> {
        let n = self.doc(id)?.pdf.pages().len();
        let mut map = serde_json::Map::new();
        for i in 0..n {
            for f in self.list_fields(id, i as u32)? {
                let v = match f.kind.as_str() {
                    "checkbox" => serde_json::Value::Bool(f.checked),
                    "radio" => {
                        if f.checked {
                            serde_json::Value::String(f.value.clone())
                        } else {
                            continue;
                        }
                    }
                    "button" | "signature" => continue,
                    _ => serde_json::Value::String(f.value.clone()),
                };
                map.insert(f.name.clone(), v);
            }
        }
        Ok(serde_json::Value::Object(map))
    }

    pub fn import_form_data(&mut self, id: u32, data: serde_json::Value) -> Result<u32> {
        let obj = data.as_object().cloned().ok_or_else(|| EngineError::invalid("expected a JSON object"))?;
        let n = self.doc(id)?.pdf.pages().len();
        let mut fields = Vec::new();
        for i in 0..n {
            fields.extend(self.list_fields(id, i as u32)?);
        }
        let mut count = 0;
        for f in fields {
            let Some(v) = obj.get(&f.name) else { continue };
            let fv = match (f.kind.as_str(), v) {
                ("checkbox", serde_json::Value::Bool(b)) => FieldValue::Check { checked: *b },
                ("radio", serde_json::Value::String(s)) if *s == f.value => FieldValue::Check { checked: true },
                ("radio", _) => continue,
                ("combo" | "list", serde_json::Value::String(s)) => FieldValue::Choice { value: s.clone() },
                ("text", serde_json::Value::String(s)) => FieldValue::Text { value: s.clone() },
                ("text", other) => FieldValue::Text { value: other.to_string() },
                _ => continue,
            };
            self.set_field(id, f.page, f.annot_index, fv)?;
            count += 1;
        }
        Ok(count)
    }
}
