//! Local package registry and installed package metadata management.

use crate::config::AsterConfig;
use crate::error::Result;
use crate::schema::{validate_binary_name, validate_package_id, validate_registry};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct RegistryManager {
    pub config: AsterConfig,
}

impl RegistryManager {
    pub fn new(config: AsterConfig) -> Result<Self> {
        config.ensure_directories()?;
        let manager = RegistryManager { config };
        manager.ensure_registry_exists()?;
        Ok(manager)
    }

    fn ensure_registry_exists(&self) -> Result<()> {
        if !self.config.packages_json.exists() {
            let initial = json!({
                "schema_version": 1,
                "installed": {}
            });
            self.config
                .save_json_atomic(&self.config.packages_json, &initial)?;
        }
        Ok(())
    }

    /// Loads and validates `packages.json`.
    pub fn load_registry(&self) -> Result<Map<String, Value>> {
        let data = self.config.load_json(&self.config.packages_json)?;
        if data.is_empty() {
            let mut map = Map::new();
            map.insert("schema_version".to_string(), json!(1));
            map.insert("installed".to_string(), Value::Object(Map::new()));
            self.config
                .save_json_atomic(&self.config.packages_json, &Value::Object(map.clone()))?;
            Ok(map)
        } else {
            validate_registry(&Value::Object(data.clone()))?;
            Ok(data)
        }
    }

    /// Saves registry data atomically after validation.
    pub fn save_registry(&self, registry_data: &Map<String, Value>) -> Result<()> {
        validate_registry(&Value::Object(registry_data.clone()))?;
        self.config.save_json_atomic(
            &self.config.packages_json,
            &Value::Object(registry_data.clone()),
        )
    }

    /// Checks whether a package ID is recorded as installed.
    pub fn is_installed(&self, package_id: &str) -> Result<bool> {
        let registry = self.load_registry()?;
        Ok(installed_map(&registry).contains_key(package_id))
    }

    /// Returns the registry entry for an installed package, if present.
    pub fn get_installed_package(&self, package_id: &str) -> Result<Option<Value>> {
        let registry = self.load_registry()?;
        Ok(installed_map(&registry).get(package_id).cloned())
    }

    /// Returns all installed package records.
    pub fn list_installed(&self) -> Result<Map<String, Value>> {
        let registry = self.load_registry()?;
        Ok(installed_map(&registry).clone())
    }

    /// Registers an installed package in `packages.json` and writes `metadata.json`.
    #[allow(clippy::too_many_arguments)]
    pub fn register_package(
        &self,
        package_id: &str,
        name: &str,
        version: &str,
        pkg_type: &str,
        installed_files: &[String],
        provided_binaries: &[String],
        source_repository: &str,
        extra_metadata: Option<&Map<String, Value>>,
    ) -> Result<()> {
        validate_package_id(package_id)?;
        for binary_name in provided_binaries {
            validate_binary_name(binary_name)?;
        }
        let package_dir = self.config.packages_dir.join(package_id);
        std::fs::create_dir_all(&package_dir)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let mut metadata = Map::new();
        metadata.insert("schema_version".to_string(), json!(1));
        metadata.insert("id".to_string(), json!(package_id));
        metadata.insert("name".to_string(), json!(name));
        metadata.insert("version".to_string(), json!(version));
        metadata.insert("type".to_string(), json!(pkg_type));
        metadata.insert("source_repository".to_string(), json!(source_repository));
        metadata.insert(
            "installation_time".to_string(),
            Value::Number(
                serde_json::Number::from_f64(now).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );
        metadata.insert("installed_files".to_string(), json!(installed_files));
        metadata.insert("provided_binaries".to_string(), json!(provided_binaries));

        if let Some(extra) = extra_metadata {
            for (k, v) in extra {
                metadata.insert(k.clone(), v.clone());
            }
        }

        let metadata_file = package_dir.join("metadata.json");
        self.config
            .save_json_atomic(&metadata_file, &Value::Object(metadata))?;

        let mut registry = self.load_registry()?;
        let install_dir: PathBuf = package_dir
            .strip_prefix(&self.config.root_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| package_dir.clone());

        let entry = json!({
            "name": name,
            "version": version,
            "type": pkg_type,
            "install_dir": install_dir.to_string_lossy().to_string(),
            "installed_files": installed_files,
            "provided_binaries": provided_binaries,
            "source_repository": source_repository,
        });

        let installed = registry
            .entry("installed".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = installed {
            map.insert(package_id.to_string(), entry);
        }
        self.save_registry(&registry)
    }

    /// Verifies intactness of an installed package, returning a list of issues found.
    pub fn verify_package(&self, package_id: &str) -> Result<Vec<String>> {
        validate_package_id(package_id)?;
        let info = match self.get_installed_package(package_id)? {
            Some(i) => i,
            None => {
                return Err(crate::error::AsterError::Runtime(format!(
                    "Package '{package_id}' is not installed."
                )))
            }
        };

        let mut issues = Vec::new();
        let pkg_dir = self.config.packages_dir.join(package_id);
        let pkg_resolved = std::fs::canonicalize(&pkg_dir).unwrap_or_else(|_| pkg_dir.clone());

        let package_dir_is_real = match std::fs::symlink_metadata(&pkg_dir) {
            Ok(metadata) if metadata.file_type().is_dir() => true,
            Ok(_) => {
                issues.push(format!(
                    "Package path is not a real directory: {}",
                    pkg_dir.display()
                ));
                false
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                issues.push(format!("Missing package directory: {}", pkg_dir.display()));
                false
            }
            Err(e) => {
                issues.push(format!(
                    "Cannot inspect package directory '{}': {e}",
                    pkg_dir.display()
                ));
                false
            }
        };

        if package_dir_is_real {
            match info.get("installed_files").and_then(|v| v.as_array()) {
                Some(files) => {
                    for f in files {
                        match f.as_str() {
                            Some(rel) if !is_safe_recorded_file_path(rel) => {
                                issues.push(format!("Unsafe recorded file path: {rel}"));
                            }
                            Some(rel) => {
                                let fpath = pkg_dir.join(rel);
                                if !crate::util::exists_or_symlink(&fpath) {
                                    issues.push(format!("Missing recorded file: {rel}"));
                                } else if fpath.is_symlink() && !fpath.exists() {
                                    issues.push(format!("Broken recorded symlink: {rel}"));
                                }
                            }
                            None => issues.push(
                                "Invalid recorded file entry: expected a relative path string"
                                    .to_string(),
                            ),
                        }
                    }
                }
                None => issues.push("Missing or invalid installed file metadata".to_string()),
            }
        }

        match info.get("provided_binaries").and_then(|v| v.as_array()) {
            Some(bins) => {
                for b in bins {
                    let bin_name = match b.as_str() {
                        Some(name) if validate_binary_name(name).is_ok() => {
                            name
                        }
                        Some(name) => {
                            issues.push(format!("Unsafe recorded command name: {name}"));
                            continue;
                        }
                        None => {
                            issues.push(
                                "Invalid recorded command entry: expected a command name string"
                                    .to_string(),
                            );
                            continue;
                        }
                    };

                    let link_path = self.config.bin_dir.join(bin_name);
                    if !crate::util::exists_or_symlink(&link_path) {
                        issues.push(format!("Missing command symlink: bin/{bin_name}"));
                    } else if !link_path.exists() {
                        issues.push(format!("Broken command symlink: bin/{bin_name}"));
                    } else {
                        let belongs = match std::fs::canonicalize(&link_path) {
                            Ok(target) => {
                                target.starts_with(&pkg_resolved) || target.starts_with(&pkg_dir)
                            }
                            Err(_) => {
                                if let Ok(raw_target) = std::fs::read_link(&link_path) {
                                    let raw_path = if raw_target.is_relative() {
                                        crate::util::normalize_path_lexically(
                                            &self.config.bin_dir.join(&raw_target),
                                        )
                                    } else {
                                        crate::util::normalize_path_lexically(&raw_target)
                                    };
                                    raw_path.starts_with(&pkg_resolved)
                                        || raw_path.starts_with(&pkg_dir)
                                } else {
                                    false
                                }
                            }
                        };
                        if !belongs {
                            issues.push(format!(
                                "Command symlink bin/{bin_name} no longer points to package '{package_id}'"
                            ));
                        }
                    }
                }
            }
            None => issues.push("Missing or invalid provided command metadata".to_string()),
        }

        Ok(issues)
    }

    /// Removes a package from `packages.json`.
    pub fn unregister_package(&self, package_id: &str) -> Result<()> {
        validate_package_id(package_id)?;
        let mut registry = self.load_registry()?;
        if let Some(Value::Object(map)) = registry.get_mut("installed") {
            if map.remove(package_id).is_some() {
                self.save_registry(&registry)?;
            }
        }
        Ok(())
    }
}

fn is_safe_recorded_file_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn installed_map(registry: &Map<String, Value>) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    registry
        .get("installed")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}
