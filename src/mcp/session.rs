use crate::mcp::fs_guard::FsGuard;
use crate::parser::FileFormat;
use crate::DocumentCore;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct DocumentSession {
    pub id: String,
    pub core: DocumentCore,
    pub path: Option<PathBuf>,
    pub source_format: FileFormat,
    pub dirty: bool,
}

impl DocumentSession {
    pub fn metadata_json(&self) -> serde_json::Value {
        json!({
            "session_id": &self.id,
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "source_format": format_name(self.source_format),
            "dirty": self.dirty,
            "page_count": self.core.page_count(),
        })
    }
}

pub struct SessionManager {
    sessions: HashMap<String, DocumentSession>,
    next_id: u64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn open(&mut self, guard: &FsGuard, path: &str) -> Result<serde_json::Value, String> {
        let resolved = guard.resolve_existing_file(path)?;
        let bytes = fs::read(&resolved)
            .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
        let source_format = crate::parser::detect_format(&bytes);
        let core = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let id = self.allocate_id();
        let session = DocumentSession {
            id: id.clone(),
            core,
            path: Some(resolved.clone()),
            source_format,
            dirty: false,
        };
        let info = info_value(&session.core);
        let metadata = session.metadata_json();
        self.sessions.insert(id.clone(), session);
        Ok(json!({
            "session_id": id,
            "path": resolved.display().to_string(),
            "source_format": format_name(source_format),
            "document_info": info,
            "metadata": metadata,
        }))
    }

    pub fn new_document(&mut self) -> Result<serde_json::Value, String> {
        let mut core = DocumentCore::new_empty();
        core.create_blank_document_native()
            .map_err(|e| e.to_string())?;
        self.new_core_session(core, FileFormat::Hwp, true)
    }

    pub fn new_core_session(
        &mut self,
        core: DocumentCore,
        source_format: FileFormat,
        dirty: bool,
    ) -> Result<serde_json::Value, String> {
        let id = self.allocate_id();
        let session = DocumentSession {
            id: id.clone(),
            core,
            path: None,
            source_format,
            dirty,
        };
        let info = info_value(&session.core);
        let metadata = session.metadata_json();
        self.sessions.insert(id.clone(), session);
        Ok(json!({
            "session_id": id,
            "source_format": format_name(source_format),
            "document_info": info,
            "dirty": dirty,
            "metadata": metadata,
        }))
    }

    pub fn close(&mut self, id: &str) -> Result<serde_json::Value, String> {
        let session = self
            .sessions
            .remove(id)
            .ok_or_else(|| format!("unknown session_id: {id}"))?;
        Ok(json!({
            "ok": true,
            "session_id": id,
            "dirty": session.dirty,
        }))
    }

    pub fn get(&self, id: &str) -> Result<&DocumentSession, String> {
        self.sessions
            .get(id)
            .ok_or_else(|| format!("unknown session_id: {id}"))
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut DocumentSession, String> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| format!("unknown session_id: {id}"))
    }

    fn allocate_id(&mut self) -> String {
        let id = format!("rhwp-{}", self.next_id);
        self.next_id += 1;
        id
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn info_value(core: &DocumentCore) -> serde_json::Value {
    serde_json::from_str(&core.get_document_info()).unwrap_or_else(|_| {
        json!({
            "pageCount": core.page_count(),
        })
    })
}

pub fn format_name(format: FileFormat) -> &'static str {
    match format {
        FileFormat::Hwp => "hwp",
        FileFormat::Hwpx => "hwpx",
        FileFormat::Hwp3 => "hwp3",
        FileFormat::LegacyHwpml => "legacy-hwpml",
        FileFormat::Unknown => "unknown",
    }
}
