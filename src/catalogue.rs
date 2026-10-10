//! Catalogue repository management and fetching.

use crate::config::{fetch_url, AsterConfig};
use crate::error::{AsterError, Result};
use crate::schema::{validate_index, validate_package_definition};
use crate::util::py_str;
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

#[derive(Clone)]
pub struct CatalogueManager {
    pub config: AsterConfig,
}

impl CatalogueManager {
    pub fn new(config: AsterConfig) -> Result<Self> {
        config.ensure_directories()?;
        Ok(CatalogueManager { config })
    }

    /// Loads configured catalogue repositories from `repositories.json`.
    pub fn get_repositories(&self) -> Result<Map<String, Value>> {
        let data = self.config.load_json(&self.config.repositories_json)?;
        Ok(data
            .get("repositories")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default())
    }

    /// Returns `(repo_name, repo_info)` sorted by priority, highest first.
    pub fn get_sorted_repositories(&self) -> Result<Vec<(String, Value)>> {
        let repos = self.get_repositories()?;
        let mut items: Vec<(String, Value)> = repos.into_iter().collect();
        items.sort_by(|a, b| {
            priority_of(&b.1)
                .partial_cmp(&priority_of(&a.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(items)
    }

    /// Fetches the remote `index.json` for `repo_name` and caches it locally.
    pub fn update_repository(&self, repo_name: &str) -> Result<Value> {
        let repos = self.get_repositories()?;
        let repo_info = repos.get(repo_name).ok_or_else(|| {
            AsterError::Runtime(format!("Repository '{repo_name}' is not configured."))
        })?;

        let repo_url = repo_info
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();
        let index_url = format!("{repo_url}/index.json");

        let cache_dir = self.config.repository_cache.join(repo_name);
        fs::create_dir_all(&cache_dir)?;
        let cached_index_file = cache_dir.join("index.json");

        let data: Value = if repo_url.starts_with("http://") || repo_url.starts_with("https://") {
            let raw = fetch_url(&index_url, None, 15).map_err(|e| {
                AsterError::Runtime(format!(
                    "Failed to fetch repository index from {index_url}: {e}"
                ))
            })?;
            serde_json::from_slice(&raw).map_err(AsterError::Json)?
        } else if let Some(rest) = repo_url.strip_prefix("file://") {
            let local_path = Path::new(rest).join("index.json");
            if !local_path.exists() {
                return Err(AsterError::NotFound(format!(
                    "Local catalogue index file not found: {}",
                    local_path.display()
                )));
            }
            let text = fs::read_to_string(&local_path)?;
            serde_json::from_str(&text).map_err(AsterError::Json)?
        } else {
            let local_path = Path::new(&repo_url).join("index.json");
            if !local_path.exists() {
                return Err(AsterError::NotFound(format!(
                    "Local catalogue index file not found: {}",
                    local_path.display()
                )));
            }
            let text = fs::read_to_string(&local_path)?;
            serde_json::from_str(&text).map_err(AsterError::Json)?
        };

        validate_index(&data)?;
        self.config.save_json_atomic(&cached_index_file, &data)?;

        let pkgs_cache_dir = cache_dir.join("packages");
        if pkgs_cache_dir.exists() {
            fs::remove_dir_all(&pkgs_cache_dir)?;
        }

        Ok(data)
    }

    /// Updates all configured repositories.
    pub fn update_all(&self) -> Result<()> {
        let repos = self.get_repositories()?;
        for repo_name in repos.keys() {
            self.update_repository(repo_name)?;
        }
        Ok(())
    }

    /// Returns the cached `index.json` for `repo_name` if present.
    pub fn get_cached_index(&self, repo_name: &str) -> Result<Option<Value>> {
        let cached = self
            .config
            .repository_cache
            .join(repo_name)
            .join("index.json");
        if !cached.exists() {
            return Ok(None);
        }
        let map = self.config.load_json(&cached)?;
        Ok(Some(Value::Object(map)))
    }

    /// Searches across cached indexes in descending priority order.
    pub fn search_packages(&self, query: &str) -> Result<Map<String, Value>> {
        let query_lower = query.to_lowercase();
        let mut results: Map<String, Value> = Map::new();
        for (repo_name, _) in self.get_sorted_repositories()? {
            let index = match self.get_cached_index(&repo_name)? {
                Some(idx) => idx,
                None => continue,
            };
            let pkgs = match index.get("packages").and_then(|v| v.as_object()) {
                Some(p) => p,
                None => continue,
            };
            for (pkg_id, pkg_info) in pkgs {
                if results.contains_key(pkg_id) {
                    continue;
                }
                let desc = pkg_info
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if query_lower.is_empty()
                    || pkg_id.to_lowercase().contains(&query_lower)
                    || desc.to_lowercase().contains(&query_lower)
                {
                    let mut entry = pkg_info.as_object().cloned().unwrap_or_default();
                    entry.insert("repository".to_string(), Value::String(repo_name.clone()));
                    results.insert(pkg_id.clone(), Value::Object(entry));
                }
            }
        }
        Ok(results)
    }

    /// Fetches the package definition JSON for `package_id`.
    pub fn get_package_definition(
        &self,
        package_id: &str,
        repo_name: Option<&str>,
    ) -> Result<Option<Value>> {
        let repos = self.get_repositories()?;
        let target_repos: Vec<(String, Value)> = match repo_name {
            Some(name) => match repos.get(name) {
                Some(info) => vec![(name.to_string(), info.clone())],
                None => vec![],
            },
            None => self.get_sorted_repositories()?,
        };

        for (rname, _) in target_repos {
            let index = match self.get_cached_index(&rname)? {
                Some(idx) => idx,
                None => continue,
            };
            let pkgs = match index.get("packages").and_then(|v| v.as_object()) {
                Some(p) => p,
                None => continue,
            };
            let pkg_entry = match pkgs.get(package_id) {
                Some(e) => e,
                None => continue,
            };

            let def_path = pkg_entry
                .get("definition")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("packages/{package_id}.json"));

            let cached_def_file = self.config.repository_cache.join(&rname).join(&def_path);
            if cached_def_file.exists() {
                let map = self.config.load_json(&cached_def_file)?;
                let data = Value::Object(map);
                validate_package_definition(&data)?;
                return Ok(Some(data));
            }

            let repo_url = repos
                .get(&rname)
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim_end_matches('/')
                .to_string();
            let def_url = format!("{repo_url}/{def_path}");

            let data: Value = if repo_url.starts_with("http://") || repo_url.starts_with("https://")
            {
                let raw = fetch_url(&def_url, None, 15).map_err(|e| {
                    AsterError::Runtime(format!(
                        "Failed to fetch package definition from {def_url}: {e}"
                    ))
                })?;
                serde_json::from_slice(&raw).map_err(AsterError::Json)?
            } else {
                let base = repo_url.strip_prefix("file://").unwrap_or(&repo_url);
                let local_def = Path::new(base).join(&def_path);
                if !local_def.exists() {
                    return Err(AsterError::NotFound(format!(
                        "Local package definition file not found: {}",
                        local_def.display()
                    )));
                }
                let text = fs::read_to_string(&local_def)?;
                serde_json::from_str(&text).map_err(AsterError::Json)?
            };

            validate_package_definition(&data)?;
            self.config.save_json_atomic(&cached_def_file, &data)?;
            return Ok(Some(data));
        }

        Ok(None)
    }
}

fn priority_of(info: &Value) -> f64 {
    match info.get("priority") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(100.0),
        Some(Value::String(s)) => s.parse::<i64>().map(|i| i as f64).unwrap_or(100.0),
        _ => 100.0,
    }
}

/// Renders a priority value the way Python would for display.
pub fn display_priority(info: &Value) -> String {
    match info.get("priority") {
        Some(v) => py_str(v),
        None => "100".to_string(),
    }
}
