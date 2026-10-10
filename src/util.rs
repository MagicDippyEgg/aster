//! Small helpers shared across Aster modules.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Renders a JSON value the way Python's `str()` would for common cases.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python-like truthiness for JSON values when used in `if` conditions.
pub fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().map(|f| f != 0.0).unwrap_or(false)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Returns the value at `key` or `default` when missing, applying Python truthiness.
pub fn py_get_bool(map: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    match map.get(key) {
        Some(v) => py_truthy(v),
        None => default,
    }
}

#[cfg(unix)]
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn is_executable(_p: &Path) -> bool {
    true
}

#[cfg(unix)]
pub fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
pub fn set_mode(_p: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Mirrors Python's `Path.exists() or Path.is_symlink()` expression.
pub fn exists_or_symlink(p: &Path) -> bool {
    p.exists() || p.symlink_metadata().is_ok()
}

/// Recursively collects relative file paths below `root` (as strings with `/`).
pub fn collect_files(root: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    collect_files_inner(root, root, &mut files)?;
    Ok(files)
}

fn collect_files_inner(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_files_inner(root, &path, out)?;
        } else if ft.is_file() || ft.is_symlink() {
            if let Ok(rel) = path.strip_prefix(root) {
                let s = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.push(s);
            }
        }
    }
    Ok(())
}

/// Finds a file by name under `root`, returning the first match.
pub fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    fn walk(root: &Path, name: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let ft = entry.file_type().ok()?;
            if ft.is_dir() {
                if let Some(found) = walk(&path, name) {
                    return Some(found);
                }
            } else if entry.file_name().to_string_lossy() == name && path.is_file() {
                return Some(path);
            }
        }
        None
    }
    walk(root, name)
}

/// Locates `name` in a `:`-separated PATH, mirroring Python's `shutil.which`.
pub fn which(name: &str, path: Option<&str>) -> Option<PathBuf> {
    let path = match path {
        Some(p) => p.to_string(),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}
