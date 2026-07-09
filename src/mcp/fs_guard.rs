use serde_json::json;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FsGuard {
    root: PathBuf,
}

impl FsGuard {
    pub fn from_env_or_cwd() -> Result<Self, String> {
        let root = match env::var_os("RHWP_MCP_ROOT") {
            Some(value) => PathBuf::from(value),
            None => env::current_dir().map_err(|e| format!("current_dir failed: {e}"))?,
        };
        let root = root
            .canonicalize()
            .map_err(|e| format!("MCP root does not exist or is not accessible: {e}"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_existing_file(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = self.join_root(path)?;
        let canonical = candidate
            .canonicalize()
            .map_err(|e| format!("file is not accessible: {path}: {e}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!(
                "path escapes RHWP_MCP_ROOT: {}",
                canonical.display()
            ));
        }
        if !canonical.is_file() {
            return Err(format!("path is not a file: {}", canonical.display()));
        }
        Ok(canonical)
    }

    pub fn resolve_target_file(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = self.join_root(path)?;
        let file_name = candidate
            .file_name()
            .ok_or_else(|| "target path must include a file name".to_string())?;
        let parent = candidate
            .parent()
            .ok_or_else(|| "target path must include a parent directory".to_string())?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("target parent is not accessible: {e}"))?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(format!(
                "target path escapes RHWP_MCP_ROOT: {}",
                canonical_parent.display()
            ));
        }
        Ok(canonical_parent.join(file_name))
    }

    pub fn atomic_write(
        &self,
        target: &Path,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<serde_json::Value, String> {
        let target = self.resolve_target_file(
            target
                .to_str()
                .ok_or_else(|| "target path is not valid UTF-8".to_string())?,
        )?;
        if target.exists() && !overwrite {
            return Err(format!(
                "target exists; pass overwrite=true to replace it: {}",
                target.display()
            ));
        }
        let parent = target
            .parent()
            .ok_or_else(|| "target path has no parent".to_string())?;
        let file_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| "target path has invalid file name".to_string())?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = parent.join(format!(
            ".{file_name}.rhwp-mcp-{}-{nonce}.tmp",
            std::process::id(),
        ));

        let write_result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| format!("failed to create temp file: {e}"))?;
            file.write_all(bytes)
                .map_err(|e| format!("failed to write temp file: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("failed to sync temp file: {e}"))?;
            fs::rename(&tmp, &target).map_err(|e| format!("failed to replace target: {e}"))?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        write_result?;

        Ok(json!({
            "path": target.display().to_string(),
            "bytes": bytes.len(),
            "overwrote": overwrite,
        }))
    }

    fn join_root(&self, path: &str) -> Result<PathBuf, String> {
        if path.trim().is_empty() {
            return Err("path must not be empty".to_string());
        }
        let raw = PathBuf::from(path);
        let candidate = if raw.is_absolute() {
            raw
        } else {
            self.root.join(raw)
        };
        if candidate
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err("path must not contain '..' components".to_string());
        }
        Ok(candidate)
    }
}
