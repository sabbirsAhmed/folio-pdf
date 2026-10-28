use pdfium_render::prelude::PdfiumError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl EngineError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        EngineError { code: code.into(), message: message.into() }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for EngineError {}

impl From<PdfiumError> for EngineError {
    fn from(e: PdfiumError) -> Self {
        EngineError::new("pdfium", format!("{e:?}"))
    }
}

impl From<lopdf::Error> for EngineError {
    fn from(e: lopdf::Error) -> Self {
        EngineError::new("pdf_structure", e.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::new("io", e.to_string())
    }
}

impl From<image::ImageError> for EngineError {
    fn from(e: image::ImageError) -> Self {
        EngineError::new("image", e.to_string())
    }
}
