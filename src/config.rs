//! Configuration, directory management, and network fetching for Aster.

use crate::error::{AsterError, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const USER_AGENT: &str = "Aster-PackageManager/0.2.0";

pub fn default_aster_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".bin").join("aster")
}

fn expand_user(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if s == "~" {
        return PathBuf::from(std::env::var("HOME").unwrap_or_default());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest);
    }
    p.to_path_buf()
}

fn canonicalize_or_abs(p: &Path) -> PathBuf {
    if let Ok(c) = fs::canonicalize(p) {
        return c;
    }
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Fetches bytes from a URL using certificate verification.
pub fn fetch_url(
    url: &str,
    headers: Option<&HashMap<String, String>>,
    timeout: u64,
) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| AsterError::Http(e.to_string()))?;

    let mut req = client.get(url);
    match headers {
        Some(h) => {
            for (k, v) in h {
                let key = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                    .map_err(|e| AsterError::Http(e.to_string()))?;
                let val = reqwest::header::HeaderValue::from_str(v)
                    .map_err(|e| AsterError::Http(e.to_string()))?;
                req = req.header(key, val);
            }
        }
        None => {
            let ua = reqwest::header::HeaderValue::from_str(USER_AGENT)
                .map_err(|e| AsterError::Http(e.to_string()))?;
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
    }

    let resp = req.send().map_err(|e| AsterError::Http(e.to_string()))?;
    let resp = resp
        .error_for_status()
        .map_err(|e| AsterError::Http(e.to_string()))?;
    let bytes = resp.bytes().map_err(|e| AsterError::Http(e.to_string()))?;
    Ok(bytes.to_vec())
}

/// Downloads content from a URL directly to `dest_path`.
pub fn fetch_url_to_file(
    url: &str,
    dest_path: &Path,
    headers: Option<&HashMap<String, String>>,
    timeout: u64,
) -> Result<()> {
    let data = fetch_url(url, headers, timeout)?;
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest_path, data)?;
    Ok(())
}

/// Returns a copy of the environment with PyInstaller-injected library paths removed.
pub fn get_clean_env() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if env.contains_key("LD_LIBRARY_PATH_ORIG") {
        if let Some(orig) = env.get("LD_LIBRARY_PATH_ORIG").cloned() {
            env.insert("LD_LIBRARY_PATH".to_string(), orig);
        }
        env.remove("LD_LIBRARY_PATH_ORIG");
    } else if let Some(ld) = env.get("LD_LIBRARY_PATH").cloned() {
        let cleaned: Vec<&str> = ld.split(':').filter(|p| !p.contains("_MEI")).collect();
        if !cleaned.is_empty() {
            env.insert("LD_LIBRARY_PATH".to_string(), cleaned.join(":"));
        } else {
            env.remove("LD_LIBRARY_PATH");
        }
    }
    env
}

fn push_if_exists(paths: &mut Vec<String>, p: PathBuf) {
    if p.exists() {
        paths.push(p.to_string_lossy().to_string());
    }
}

/// Builds a build environment configured to find Aster-installed packages and
/// optional temporary build dependency prefixes.
pub fn get_build_env(
    config: &AsterConfig,
    extra_prefix_dirs: Option<&[PathBuf]>,
) -> HashMap<String, String> {
    let mut env = get_clean_env();

    let mut prefix_dirs: Vec<PathBuf> = Vec::new();
    if let Some(extra) = extra_prefix_dirs {
        prefix_dirs.extend(extra.iter().cloned());
    }

    if config.packages_dir.exists() {
        if let Ok(rd) = fs::read_dir(&config.packages_dir) {
            for entry in rd.flatten() {
                if entry.path().is_dir() {
                    prefix_dirs.push(entry.path());
                }
            }
        }
    }

    let mut bin_paths: Vec<String> = Vec::new();
    let mut inc_paths: Vec<String> = Vec::new();
    let mut lib_paths: Vec<String> = Vec::new();
    let mut cmake_paths: Vec<String> = Vec::new();
    let mut pkgconfig_paths: Vec<String> = Vec::new();

    for p in &prefix_dirs {
        push_if_exists(&mut bin_paths, p.join("bin"));
        push_if_exists(&mut inc_paths, p.join("include"));
        push_if_exists(&mut inc_paths, p.join("usr").join("include"));
        push_if_exists(&mut lib_paths, p.join("lib"));
        push_if_exists(&mut lib_paths, p.join("lib64"));
        push_if_exists(&mut lib_paths, p.join("usr").join("lib"));
        push_if_exists(&mut lib_paths, p.join("usr").join("lib64"));
        cmake_paths.push(p.to_string_lossy().to_string());
        push_if_exists(&mut pkgconfig_paths, p.join("lib").join("pkgconfig"));
        push_if_exists(&mut pkgconfig_paths, p.join("lib64").join("pkgconfig"));
        push_if_exists(&mut pkgconfig_paths, p.join("share").join("pkgconfig"));
    }

    if !bin_paths.is_empty() {
        let current = env.get("PATH").cloned().unwrap_or_default();
        if current.is_empty() {
            env.insert("PATH".to_string(), bin_paths.join(":"));
        } else {
            env.insert(
                "PATH".to_string(),
                format!("{}:{}", bin_paths.join(":"), current),
            );
        }
    }

    if !inc_paths.is_empty() {
        let inc_str = inc_paths.join(":");
        for var in ["CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH"] {
            let curr = env.get(var).cloned().unwrap_or_default();
            if curr.is_empty() {
                env.insert(var.to_string(), inc_str.clone());
            } else {
                env.insert(var.to_string(), format!("{inc_str}:{curr}"));
            }
        }
    }

    if !lib_paths.is_empty() {
        let lib_str = lib_paths.join(":");
        for var in ["LIBRARY_PATH", "LD_LIBRARY_PATH"] {
            let curr = env.get(var).cloned().unwrap_or_default();
            if curr.is_empty() {
                env.insert(var.to_string(), lib_str.clone());
            } else {
                env.insert(var.to_string(), format!("{lib_str}:{curr}"));
            }
        }
    }

    if !cmake_paths.is_empty() {
        let cmake_str = cmake_paths.join(":");
        let curr = env.get("CMAKE_PREFIX_PATH").cloned().unwrap_or_default();
        if curr.is_empty() {
            env.insert("CMAKE_PREFIX_PATH".to_string(), cmake_str);
        } else {
            env.insert(
                "CMAKE_PREFIX_PATH".to_string(),
                format!("{cmake_str}:{curr}"),
            );
        }
    }

    if !pkgconfig_paths.is_empty() {
        let pc_str = pkgconfig_paths.join(":");
        let curr = env.get("PKG_CONFIG_PATH").cloned().unwrap_or_default();
        if curr.is_empty() {
            env.insert("PKG_CONFIG_PATH".to_string(), pc_str);
        } else {
            env.insert("PKG_CONFIG_PATH".to_string(), format!("{pc_str}:{curr}"));
        }
    }

    env
}

fn to_pretty_json<T: Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(b"    "),
    );
    value.serialize(&mut ser)?;
    Ok(buf)
}

/// Atomic JSON write using "path.tmp.<pid>" like the original implementation.
fn tmp_path_for(path: &Path) -> PathBuf {
    let fname = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let pid = std::process::id();
    match fname.rfind('.') {
        Some(idx) if idx > 0 => {
            let stem = &fname[..idx];
            path.with_file_name(format!("{stem}.tmp.{pid}"))
        }
        _ => path.with_file_name(format!("{fname}.tmp.{pid}")),
    }
}

#[derive(Clone)]
pub struct AsterConfig {
    pub root_dir: PathBuf,
    pub aster_bin: PathBuf,
    pub packages_json: PathBuf,
    pub config_json: PathBuf,
    pub repositories_json: PathBuf,
    pub bin_dir: PathBuf,
    pub packages_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub downloads_cache: PathBuf,
    pub archives_cache: PathBuf,
    pub build_dir: PathBuf,
    pub repository_cache: PathBuf,
    pub logs_dir: PathBuf,
    pub toolchains_dir: PathBuf,
    pub rust_toolchain_dir: PathBuf,
}

impl AsterConfig {
    pub fn new() -> Self {
        let root = match std::env::var("ASTER_HOME") {
            Ok(home) => expand_user(Path::new(&home)),
            Err(_) => default_aster_home(),
        };
        Self::with_root_dir(root)
    }

    pub fn with_root_dir(root_dir: PathBuf) -> Self {
        let root_dir = canonicalize_or_abs(&root_dir);
        let parts = |name: &str| root_dir.join(name);
        let includes = |name: &str| root_dir.join("cache").join(name);
        AsterConfig {
            aster_bin: parts("aster"),
            packages_json: root_dir.join("packages.json"),
            config_json: root_dir.join("config.json"),
            repositories_json: root_dir.join("repositories.json"),
            bin_dir: root_dir.join("bin"),
            packages_dir: root_dir.join("packages"),
            cache_dir: root_dir.join("cache"),
            downloads_cache: includes("downloads"),
            archives_cache: includes("archives"),
            build_dir: root_dir.join("build"),
            repository_cache: root_dir.join("repository-cache"),
            logs_dir: root_dir.join("logs"),
            toolchains_dir: root_dir.join("cache").join("toolchains"),
            rust_toolchain_dir: root_dir.join("cache").join("toolchains").join("rust"),
            root_dir,
        }
    }

    /// Creates all required directories if they don't exist.
    pub fn ensure_directories(&self) -> Result<()> {
        let directories = [
            &self.root_dir,
            &self.bin_dir,
            &self.packages_dir,
            &self.cache_dir,
            &self.downloads_cache,
            &self.archives_cache,
            &self.build_dir,
            &self.repository_cache,
            &self.logs_dir,
        ];
        for d in directories {
            fs::create_dir_all(d)?;
        }

        if !self.config_json.exists() {
            let default_config = json!({
                "schema_version": 1,
                "default_repository": "default",
            });
            self.save_json_atomic(&self.config_json, &default_config)?;
        }

        if !self.repositories_json.exists() {
            let default_repos = json!({
                "schema_version": 1,
                "repositories": {
                    "default": {
                        "name": "default",
                        "url": "https://raw.githubusercontent.com/MagicDippyEgg/aster-package-repository/main",
                        "priority": 100
                    }
                }
            });
            self.save_json_atomic(&self.repositories_json, &default_repos)?;
        }

        Ok(())
    }

    /// Atomically saves data as JSON to `path`.
    pub fn save_json_atomic<T: Serialize>(&self, path: &Path, data: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = tmp_path_for(path);
        let bytes = to_pretty_json(data).map_err(AsterError::Json)?;
        fs::write(&tmp_path, bytes)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Safely loads JSON data from `path`, returning an empty object when missing.
    pub fn load_json(&self, path: &Path) -> Result<Map<String, Value>> {
        if !path.exists() {
            return Ok(Map::new());
        }
        let text = fs::read_to_string(path)?;
        let value: Value = serde_json::from_str(&text)?;
        match value {
            Value::Object(map) => Ok(map),
            _ => Ok(Map::new()),
        }
    }
}

impl Default for AsterConfig {
    fn default() -> Self {
        Self::new()
    }
}
