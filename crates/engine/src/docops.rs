//! Whole-document operations backed by lopdf, wrapped with undo checkpoints.

use crate::lo::{self, CompressLevel, CompressReport, MetadataUpdate, SanitizeOpts};
use crate::types::{DocInfo, OutlineItem};
use crate::{Result, State};

impl State {
    fn lopdf_op(&mut self, id: u32, f: impl FnOnce(&[u8]) -> Result<Vec<u8>>) -> Result<DocInfo> {
        self.checkpoint(id)?;
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let out = f(&bytes)?;
        self.replace_bytes(id, out)?;
        self.info(id)
    }

    pub fn set_metadata(&mut self, id: u32, m: MetadataUpdate) -> Result<DocInfo> {
        self.lopdf_op(id, |b| lo::set_metadata(b, &m))
    }

    pub fn set_outline(&mut self, id: u32, items: Vec<OutlineItem>) -> Result<DocInfo> {
        self.lopdf_op(id, |b| lo::set_outline(b, &items))
    }

    pub fn sanitize(&mut self, id: u32, o: SanitizeOpts) -> Result<DocInfo> {
        self.lopdf_op(id, |b| lo::sanitize(b, o))
    }

    pub fn compress(&mut self, id: u32, level: CompressLevel) -> Result<CompressReport> {
        let bytes = self.doc(id)?.pdf.save_to_bytes()?;
        let (out, report) = lo::compress(&bytes, level)?;
        if out.len() < bytes.len() {
            self.checkpoint(id)?;
            self.replace_bytes(id, out)?;
        }
        Ok(report)
    }
}
