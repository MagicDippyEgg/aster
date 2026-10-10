//! Installer for binary and source packages.

use crate::catalogue::CatalogueManager;
use crate::config::{fetch_url_to_file, get_build_env, AsterConfig};
use crate::error::{AsterError, Result};
use crate::registry::RegistryManager;
use crate::resolver::DependencyResolver;
use crate::util::{
    collect_files, exists_or_symlink, find_file, is_executable, py_get_bool, py_str, set_mode,
    which,
};
use flate2::read::GzDecoder;
use regex::Regex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use xz2::read::XzDecoder;

/// Output captured from a subprocess invocation.
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Abstraction over process execution so tests can inject a fake runner.
pub trait CommandExecutor: Send + Sync {
    fn run(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput>;

    fn run_shell(
        &self,
        script: &str,
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput>;

    fn which(&self, name: &str, path: Option<&str>) -> Option<PathBuf>;
}

/// Default executor backed by `std::process::Command`.
pub struct SystemExecutor;

impl CommandExecutor for SystemExecutor {
    fn run(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput> {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.env_clear().envs(env);
        let output = cmd.output()?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn run_shell(
        &self,
        script: &str,
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.env_clear().envs(env);
        let output = cmd.output()?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn which(&self, name: &str, path: Option<&str>) -> Option<PathBuf> {
        which(name, path)
    }
}

/// Detects platform string e.g. `linux-x86_64` or `linux-aarch64`.
pub fn get_platform_key() -> String {
    let sys_name = match std::env::consts::OS {
        "macos" => "darwin".to_string(),
        other => other.to_string(),
    };
    let machine = std::env::consts::ARCH.to_lowercase();
    let arch = match machine.as_str() {
        "x86_64" | "amd64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "aarch64".to_string(),
        other => other.to_string(),
    };
    format!("{sys_name}-{arch}")
}

#[derive(Clone)]
pub struct PackageInstaller {
    pub config: AsterConfig,
    pub registry: RegistryManager,
    pub catalogue: CatalogueManager,
    executor: Arc<dyn CommandExecutor>,
}

impl PackageInstaller {
    pub fn new(
        config: AsterConfig,
        registry: RegistryManager,
        catalogue: CatalogueManager,
    ) -> Self {
        PackageInstaller {
            config,
            registry,
            catalogue,
            executor: Arc::new(SystemExecutor),
        }
    }

    pub fn with_executor(
        config: AsterConfig,
        registry: RegistryManager,
        catalogue: CatalogueManager,
        executor: Arc<dyn CommandExecutor>,
    ) -> Self {
        PackageInstaller {
            config,
            registry,
            catalogue,
            executor,
        }
    }

    /// Installs a package and its dependencies by ID.
    pub fn install(&self, package_id: &str, auto_yes: bool) -> Result<()> {
        let resolver = DependencyResolver::new(self.catalogue.clone(), self.registry.clone());
        let install_plan = resolver
            .resolve_dependencies(package_id)
            .map_err(|e| match e {
                AsterError::Dependency(m) => {
                    AsterError::Runtime(format!("Dependency resolution failed: {m}"))
                }
                other => other,
            })?;

        for pkg in install_plan {
            if self.registry.is_installed(&pkg)? {
                if pkg == package_id {
                    let info = self.registry.get_installed_package(package_id)?;
                    let ver = info
                        .as_ref()
                        .and_then(|v| v.get("version"))
                        .map(py_str)
                        .unwrap_or_else(|| "None".to_string());
                    println!("Package '{package_id}' is already installed (version {ver}).");
                }
                continue;
            }

            let pkg_def = self.catalogue.get_package_definition(&pkg, None)?;
            let pkg_def = pkg_def.ok_or_else(|| {
                AsterError::Runtime(format!(
                    "Package definition for '{pkg}' not found in catalogue."
                ))
            })?;

            let pkg_type = pkg_def
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            match pkg_type.as_str() {
                "binary" => self.install_binary(&pkg_def)?,
                "source" => self.install_source(&pkg_def, auto_yes)?,
                other => {
                    return Err(AsterError::Runtime(format!(
                        "Unsupported package type '{other}' for package '{pkg}'."
                    )))
                }
            }
        }
        Ok(())
    }

    fn handle_build_failure(&self, output: &str, stage_name: &str) -> AsterError {
        let header_re =
            Regex::new(r"fatal error:\s*([^\s:]+\.h):\s*No such file or directory").unwrap();
        let find_re = Regex::new(r"(?i)Could NOT find ([^\s]+)\s+\(missing:").unwrap();
        let pkg_re = Regex::new(r#"Package ['"]?([^'"\s]+)['"]? not found"#).unwrap();

        let missing_headers: Vec<String> = header_re
            .captures_iter(output)
            .map(|c| c[1].to_string())
            .collect();

        let mut missing_pkgconfigs: Vec<String> = find_re
            .captures_iter(output)
            .map(|c| c[1].to_string())
            .collect();
        if missing_pkgconfigs.is_empty() {
            missing_pkgconfigs = pkg_re
                .captures_iter(output)
                .map(|c| c[1].to_string())
                .collect();
        }

        let mut msg: Vec<String> = vec![format!("{stage_name} failed.\n")];

        if !missing_headers.is_empty() || !missing_pkgconfigs.is_empty() {
            msg.push(
                "DIAGNOSTIC HINT: Missing system compilation prerequisites detected.".to_string(),
            );

            if !missing_headers.is_empty() {
                let mut unique_headers: Vec<String> = missing_headers.clone();
                unique_headers.sort();
                unique_headers.dedup();
                msg.push(format!(
                    "  Missing Header(s): {}",
                    unique_headers.join(", ")
                ));

                let mut hints: Vec<String> = Vec::new();
                for header in &unique_headers {
                    if header.contains("vulkan") {
                        hints.push("vulkan development package (e.g., 'libvulkan-dev' on Ubuntu/Debian, 'vulkan-headers' on Fedora/Arch)".to_string());
                    } else if header.contains("wayland") {
                        hints.push("wayland development package (e.g., 'libwayland-dev' or 'wayland-protocols')".to_string());
                    } else if header.contains("x11") || header.contains("X11") {
                        hints.push(
                            "X11 development package (e.g., 'libx11-dev' or 'libxcb1-dev')"
                                .to_string(),
                        );
                    } else if header.contains("pci") {
                        hints.push(
                            "pciutils development package (e.g., 'libpci-dev' or 'pciutils-devel')"
                                .to_string(),
                        );
                    }
                }
                if !hints.is_empty() {
                    msg.push(format!("  Suggested System Packages: {}", hints.join("; ")));
                }
            }

            if !missing_pkgconfigs.is_empty() {
                let mut unique_pkgs: Vec<String> = missing_pkgconfigs.clone();
                unique_pkgs.sort();
                unique_pkgs.dedup();
                msg.push(format!(
                    "  Missing Library/Module(s): {}",
                    unique_pkgs.join(", ")
                ));
            }

            msg.push(
                "\nNote: Aster does not manage or automatically install system C/C++ development libraries."
                    .to_string(),
            );
            msg.push(
                "Please install the required system development header packages using your Linux distribution's package manager."
                    .to_string(),
            );
        } else {
            let lines: Vec<String> = output
                .lines()
                .filter(|l| l.contains("error:") || l.contains("Error") || l.contains("fatal:"))
                .take(10)
                .map(|l| l.to_string())
                .collect();
            if !lines.is_empty() {
                msg.push("Error excerpt:".to_string());
                msg.extend(lines);
            }
        }

        AsterError::Runtime(msg.join("\n"))
    }

    fn check_binary_conflicts(
        &self,
        provided_binaries: &[String],
        current_package_id: &str,
    ) -> Result<()> {
        for binary_name in provided_binaries {
            let target_link = self.config.bin_dir.join(binary_name);
            if exists_or_symlink(&target_link) {
                let installed_pkgs = self.registry.list_installed()?;
                for (existing_id, existing_info) in installed_pkgs.iter() {
                    if existing_id != current_package_id {
                        let provides = existing_info
                            .get("provided_binaries")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().any(|b| b.as_str() == Some(binary_name.as_str())))
                            .unwrap_or(false);
                        if provides {
                            return Err(AsterError::Runtime(format!(
                                "Command conflict: '{binary_name}' is already provided by installed package '{existing_id}'."
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn cleanup_failed_install(&self, pkg_id: &str, provided_binaries: &[String]) {
        let staging_dir = self.config.build_dir.join(format!("staging-{pkg_id}"));
        let build_dir = self.config.build_dir.join(format!("build-{pkg_id}"));
        let final_package_dir = self.config.packages_dir.join(pkg_id);

        if staging_dir.exists() {
            let _ = std::fs::remove_dir_all(&staging_dir);
        }
        if build_dir.exists() {
            let _ = std::fs::remove_dir_all(&build_dir);
        }
        if !self.registry.is_installed(pkg_id).unwrap_or(false) && final_package_dir.exists() {
            let _ = std::fs::remove_dir_all(&final_package_dir);
        }

        if self.config.bin_dir.exists() {
            let binaries_to_check: std::collections::HashSet<&String> =
                provided_binaries.iter().collect();
            let final_resolved = std::fs::canonicalize(&final_package_dir)
                .unwrap_or_else(|_| final_package_dir.clone());
            let final_resolved_str = final_resolved.to_string_lossy().to_string();

            if let Ok(entries) = std::fs::read_dir(&self.config.bin_dir) {
                for entry in entries.flatten() {
                    let link_path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    if exists_or_symlink(&link_path) {
                        match std::fs::canonicalize(&link_path) {
                            Ok(target) => {
                                if target.to_string_lossy().starts_with(&final_resolved_str) {
                                    let _ = std::fs::remove_file(&link_path);
                                }
                            }
                            Err(_) => {
                                if link_path.is_symlink() && binaries_to_check.contains(&name) {
                                    let _ = std::fs::remove_file(&link_path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Installs a binary package from its definition.
    pub fn install_binary(&self, pkg_def: &Value) -> Result<()> {
        let pkg_id = pkg_def
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut provided_binaries: Vec<String> = Vec::new();
        let result = self.do_install_binary(pkg_def, &mut provided_binaries);
        if result.is_err() {
            self.cleanup_failed_install(&pkg_id, &provided_binaries);
        }
        result
    }

    fn do_install_binary(
        &self,
        pkg_def: &Value,
        provided_binaries: &mut Vec<String>,
    ) -> Result<()> {
        let pkg_id = pkg_def
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = pkg_def
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let name = pkg_def
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&pkg_id)
            .to_string();
        let platform_key = get_platform_key();

        let downloads = pkg_def.get("downloads").and_then(|v| v.as_object());
        let has_platform = downloads
            .map(|d| d.contains_key(&platform_key))
            .unwrap_or(false);
        if !has_platform {
            let supported: Vec<String> = match pkg_def
                .get("supported_platforms")
                .and_then(|v| v.as_array())
            {
                Some(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                None => downloads
                    .map(|d| d.keys().cloned().collect())
                    .unwrap_or_default(),
            };
            return Err(AsterError::Runtime(format!(
                "Platform '{platform_key}' is not supported by binary package '{pkg_id}'. Supported platforms: {supported:?}"
            )));
        }

        let dl_info = downloads.unwrap().get(&platform_key).unwrap();
        let url = dl_info
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let expected_sha256 = dl_info
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        println!("Downloading binary release for {pkg_id} ({platform_key})...");
        let download_filename = url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{pkg_id}.archive"));
        let dest_archive = self.config.downloads_cache.join(&download_filename);

        if url.starts_with("http://") || url.starts_with("https://") {
            if let Err(e) = fetch_url_to_file(&url, &dest_archive, None, 30) {
                if dest_archive.exists() {
                    let _ = std::fs::remove_file(&dest_archive);
                }
                return Err(AsterError::Runtime(format!(
                    "Failed to download asset from {url}: {e}"
                )));
            }
        } else {
            let src_path = if let Some(rest) = url.strip_prefix("file://") {
                PathBuf::from(rest)
            } else {
                PathBuf::from(&url)
            };
            if !src_path.exists() {
                return Err(AsterError::NotFound(format!(
                    "Local binary asset not found: {}",
                    src_path.display()
                )));
            }
            std::fs::copy(&src_path, &dest_archive)?;
        }

        if let Some(expected) = &expected_sha256 {
            if expected != "EXPECTED_SHA256" {
                let mut hasher = Sha256::new();
                let mut f = std::fs::File::open(&dest_archive)?;
                let mut buf = [0u8; 65536];
                loop {
                    let n = f.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
                let digest = format!("{:x}", hasher.finalize());
                if digest.to_lowercase() != expected.to_lowercase() {
                    if dest_archive.exists() {
                        let _ = std::fs::remove_file(&dest_archive);
                    }
                    return Err(AsterError::Runtime(format!(
                        "Integrity check failed for '{pkg_id}'. Expected sha256 {expected}, got {digest}"
                    )));
                }
            }
        }

        let staging_dir = self.config.build_dir.join(format!("staging-{pkg_id}"));
        if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir)?;
        }
        std::fs::create_dir_all(&staging_dir)?;

        println!("Extracting release archive...");
        let mut archive_format = pkg_def
            .get("archive")
            .and_then(|a| a.get("format"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if archive_format.is_none() {
            archive_format = infer_archive_format(&download_filename);
        }

        let ds = dest_archive.to_string_lossy().to_string();
        if ds.ends_with(".tar.gz")
            || ds.ends_with(".tgz")
            || ds.ends_with(".tar.xz")
            || ds.ends_with(".txz")
            || matches!(archive_format.as_deref(), Some("tar.gz") | Some("tar.xz"))
        {
            extract_tar(&dest_archive, &staging_dir)?;
        } else if ds.ends_with(".zip") || archive_format.as_deref() == Some("zip") {
            extract_zip(&dest_archive, &staging_dir)?;
        } else {
            let bin_name = pkg_id.replace("-bin", "");
            let dest_file = staging_dir.join("bin").join(&bin_name);
            if let Some(parent) = dest_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&dest_archive, &dest_file)?;
            set_mode(&dest_file, 0o755)?;
        }

        unnest_single_dir(&staging_dir)?;

        let extracted_bin_dir = staging_dir.join("bin");
        if !extracted_bin_dir.exists() {
            let executables: Vec<PathBuf> = read_dir_paths(&staging_dir)
                .into_iter()
                .filter(|p| p.is_file() && is_executable(p))
                .collect();
            if !executables.is_empty() {
                std::fs::create_dir_all(&extracted_bin_dir)?;
                for exe in executables {
                    let file_name = exe.file_name().unwrap().to_owned();
                    std::fs::rename(&exe, extracted_bin_dir.join(file_name))?;
                }
            }
        }

        if extracted_bin_dir.exists() {
            *provided_binaries = read_dir_paths(&extracted_bin_dir)
                .into_iter()
                .filter(|p| p.is_file())
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .collect();
        }

        if provided_binaries.is_empty() {
            *provided_binaries = vec![pkg_id.replace("-bin", "")];
        }

        self.check_binary_conflicts(provided_binaries, &pkg_id)?;

        let final_package_dir = self.config.packages_dir.join(&pkg_id);
        if final_package_dir.exists() {
            std::fs::remove_dir_all(&final_package_dir)?;
        }
        std::fs::rename(&staging_dir, &final_package_dir)?;

        let installed_files = collect_files(&final_package_dir)?;

        self.create_symlinks(&final_package_dir, provided_binaries)?;

        self.registry.register_package(
            &pkg_id,
            &name,
            &version,
            "binary",
            &installed_files,
            provided_binaries,
            "default",
            None,
        )?;
        println!("Successfully installed '{pkg_id}' version {version}.");
        Ok(())
    }

    /// Installs a source package by cloning/downloading and building it.
    pub fn install_source(&self, pkg_def: &Value, auto_yes: bool) -> Result<()> {
        let pkg_id = pkg_def
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut provided_binaries: Vec<String> = Vec::new();
        let result = self.do_install_source(pkg_def, auto_yes, &mut provided_binaries);
        if result.is_err() {
            self.cleanup_failed_install(&pkg_id, &provided_binaries);
        }
        result
    }

    fn do_install_source(
        &self,
        pkg_def: &Value,
        auto_yes: bool,
        provided_binaries: &mut Vec<String>,
    ) -> Result<()> {
        let pkg_id = pkg_def
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = pkg_def
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let name = pkg_def
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&pkg_id)
            .to_string();

        let source_info = pkg_def.get("source").and_then(|v| v.as_object());
        let src_type = source_info
            .and_then(|s| s.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let src_url = source_info
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        println!("Fetching source for {pkg_id}...");
        let build_dir = self.config.build_dir.join(format!("build-{pkg_id}"));
        if build_dir.exists() {
            std::fs::remove_dir_all(&build_dir)?;
        }
        std::fs::create_dir_all(&build_dir)?;

        let mut build_env = get_build_env(&self.config, None);

        match src_type.as_str() {
            "git" => {
                let git_ref = source_info
                    .and_then(|s| s.get("ref"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string();
                let cmd: Vec<String> = vec![
                    "git".to_string(),
                    "clone".to_string(),
                    "--depth".to_string(),
                    "1".to_string(),
                    "--branch".to_string(),
                    git_ref,
                    src_url.clone(),
                    build_dir.to_string_lossy().to_string(),
                ];
                let res = self.executor.run(&cmd, None, &build_env)?;
                if res.code != 0 {
                    let cmd2: Vec<String> = vec![
                        "git".to_string(),
                        "clone".to_string(),
                        "--depth".to_string(),
                        "1".to_string(),
                        src_url.clone(),
                        build_dir.to_string_lossy().to_string(),
                    ];
                    let res2 = self.executor.run(&cmd2, None, &build_env)?;
                    if res2.code != 0 {
                        return Err(AsterError::Runtime(format!(
                            "Git clone failed for '{pkg_id}': {}",
                            res2.stderr
                        )));
                    }
                }
            }
            "tar.gz" | "tar.xz" | "txz" | "zip" | "archive" | "url" => {
                let ext = if src_type == "zip" || src_url.ends_with(".zip") {
                    ".zip"
                } else if src_type == "tar.xz"
                    || src_type == "txz"
                    || src_url.ends_with(".tar.xz")
                    || src_url.ends_with(".txz")
                {
                    ".tar.xz"
                } else {
                    ".tar.gz"
                };
                let dest_archive = self.config.downloads_cache.join(format!("{pkg_id}{ext}"));

                if src_url.starts_with("http://") || src_url.starts_with("https://") {
                    fetch_url_to_file(&src_url, &dest_archive, None, 30)?;
                } else {
                    let src_path = if let Some(rest) = src_url.strip_prefix("file://") {
                        PathBuf::from(rest)
                    } else {
                        PathBuf::from(&src_url)
                    };
                    std::fs::copy(&src_path, &dest_archive)?;
                }

                let ds = dest_archive.to_string_lossy().to_string();
                if ds.ends_with(".zip") || src_type == "zip" {
                    extract_zip(&dest_archive, &build_dir)?;
                } else {
                    extract_tar(&dest_archive, &build_dir)?;
                }
            }
            other => {
                return Err(AsterError::Runtime(format!(
                    "Unsupported source type '{other}' for package '{pkg_id}'."
                )));
            }
        }

        unnest_build_dir(&build_dir)?;

        let build_info = pkg_def.get("build").and_then(|v| v.as_object());
        let build_system = build_info
            .and_then(|b| b.get("system"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let build_steps = build_info
            .and_then(|b| b.get("steps"))
            .and_then(|v| v.as_array())
            .cloned();

        let staging_dir = self.config.build_dir.join(format!("staging-{pkg_id}"));
        if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir)?;
        }
        std::fs::create_dir_all(&staging_dir)?;

        if let Some(steps) = build_steps {
            println!("\nPackage '{pkg_id}' defines custom build steps:");
            for (idx, step) in steps.iter().enumerate() {
                println!("  {}. {}", idx + 1, py_str(step));
            }

            if !auto_yes {
                let response = prompt_line("\nDo you want to execute these build steps? [y/N]: ")
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| "n".to_string());
                if response != "y" && response != "yes" {
                    println!("Installation cancelled by user.");
                    if build_dir.exists() {
                        let _ = std::fs::remove_dir_all(&build_dir);
                    }
                    if staging_dir.exists() {
                        let _ = std::fs::remove_dir_all(&staging_dir);
                    }
                    return Ok(());
                }
            }

            for step in steps {
                let step_str = py_str(&step);
                println!("Executing step: {step_str}");
                let res = self
                    .executor
                    .run_shell(&step_str, Some(&build_dir), &build_env)?;
                if res.code != 0 {
                    return Err(self.handle_build_failure(
                        &format!("{}\n{}", res.stdout, res.stderr),
                        &format!("Build step '{step_str}'"),
                    ));
                }
            }
        } else if build_system == "cargo" {
            self.build_cargo(
                &pkg_id,
                pkg_def,
                build_info,
                &build_dir,
                &staging_dir,
                &mut build_env,
                auto_yes,
            )?;
        } else if build_system == "cmake" {
            println!("Building {pkg_id} (cmake)...");
            let cmake_build_dir = build_dir.join("build_output");
            std::fs::create_dir_all(&cmake_build_dir)?;
            let configure: Vec<String> = vec![
                "cmake".to_string(),
                "-B".to_string(),
                cmake_build_dir.to_string_lossy().to_string(),
                "-S".to_string(),
                build_dir.to_string_lossy().to_string(),
                format!("-DCMAKE_INSTALL_PREFIX={}", staging_dir.to_string_lossy()),
            ];
            let res = self.executor.run(&configure, None, &build_env)?;
            if res.code != 0 {
                return Err(self.handle_build_failure(
                    &format!("{}\n{}", res.stdout, res.stderr),
                    "CMake configuration",
                ));
            }
            let build: Vec<String> = vec![
                "cmake".to_string(),
                "--build".to_string(),
                cmake_build_dir.to_string_lossy().to_string(),
            ];
            let res = self.executor.run(&build, None, &build_env)?;
            if res.code != 0 {
                return Err(self.handle_build_failure(
                    &format!("{}\n{}", res.stdout, res.stderr),
                    "CMake build",
                ));
            }
            let install: Vec<String> = vec![
                "cmake".to_string(),
                "--install".to_string(),
                cmake_build_dir.to_string_lossy().to_string(),
            ];
            let res = self.executor.run(&install, None, &build_env)?;
            if res.code != 0 {
                return Err(self.handle_build_failure(
                    &format!("{}\n{}", res.stdout, res.stderr),
                    "CMake install",
                ));
            }
        } else if build_system == "make" {
            println!("Building {pkg_id} (make)...");
            let make_cmd: Vec<String> = vec![
                "make".to_string(),
                "-C".to_string(),
                build_dir.to_string_lossy().to_string(),
            ];
            let res = self.executor.run(&make_cmd, None, &build_env)?;
            if res.code != 0 {
                return Err(self.handle_build_failure(
                    &format!("{}\n{}", res.stdout, res.stderr),
                    "Make build",
                ));
            }
            let install_cmd: Vec<String> = vec![
                "make".to_string(),
                "-C".to_string(),
                build_dir.to_string_lossy().to_string(),
                format!("DESTDIR={}", staging_dir.to_string_lossy()),
                "install".to_string(),
            ];
            let _res = self.executor.run(&install_cmd, None, &build_env)?;
        }

        let extracted_bin_dir = staging_dir.join("bin");
        if !extracted_bin_dir.exists() {
            std::fs::create_dir_all(&extracted_bin_dir)?;
            for fp in walk_files(&build_dir) {
                if fp.is_file() && is_executable(&fp) {
                    let fname = fp
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if fname.ends_with(".sh") {
                        continue;
                    }
                    std::fs::copy(&fp, extracted_bin_dir.join(&fname))?;
                }
            }
        }

        if extracted_bin_dir.exists() {
            *provided_binaries = read_dir_paths(&extracted_bin_dir)
                .into_iter()
                .filter(|p| p.is_file())
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .collect();
        }

        if provided_binaries.is_empty() {
            *provided_binaries = vec![pkg_id.clone()];
        }

        self.check_binary_conflicts(provided_binaries, &pkg_id)?;

        let final_package_dir = self.config.packages_dir.join(&pkg_id);
        if final_package_dir.exists() {
            std::fs::remove_dir_all(&final_package_dir)?;
        }
        std::fs::rename(&staging_dir, &final_package_dir)?;

        if build_dir.exists() {
            std::fs::remove_dir_all(&build_dir)?;
        }

        let installed_files = collect_files(&final_package_dir)?;

        self.create_symlinks(&final_package_dir, provided_binaries)?;

        self.registry.register_package(
            &pkg_id,
            &name,
            &version,
            "source",
            &installed_files,
            provided_binaries,
            "default",
            None,
        )?;
        println!("Successfully compiled and installed '{pkg_id}' version {version}.");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_cargo(
        &self,
        pkg_id: &str,
        pkg_def: &Value,
        build_info: Option<&Map<String, Value>>,
        build_dir: &Path,
        staging_dir: &Path,
        build_env: &mut HashMap<String, String>,
        auto_yes: bool,
    ) -> Result<()> {
        let custom_cargo_args = build_info.and_then(|b| b.get("cargo_args"));
        if let Some(args) = custom_cargo_args {
            let invalid = match args.as_array() {
                Some(arr) => {
                    arr.is_empty() || arr.iter().any(|a| a.as_str().is_none_or(|s| s.is_empty()))
                }
                None => true,
            };
            if invalid {
                return Err(AsterError::Runtime(format!(
                    "Invalid 'build.cargo_args' for package '{pkg_id}': expected a non-empty list of strings."
                )));
            }
        }

        println!("Building {pkg_id} (cargo)...");
        let system_cargo = self
            .executor
            .which("cargo", build_env.get("PATH").map(|s| s.as_str()));
        let rust_toolchain_bin = self.config.rust_toolchain_dir.join("bin");
        let mut using_isolated_toolchain = false;
        let mut downloaded_toolchain = false;
        let cargo_bin: String;

        if let Some(sc) = system_cargo {
            cargo_bin = sc.to_string_lossy().to_string();
        } else if rust_toolchain_bin.join("cargo").exists() {
            cargo_bin = rust_toolchain_bin
                .join("cargo")
                .to_string_lossy()
                .to_string();
            using_isolated_toolchain = true;
        } else {
            println!("\nAster: Cargo is required to build {pkg_id}.");
            println!("       Rust is not installed in Aster's build environment.");
            println!("       Downloading an isolated Rust toolchain...");

            std::fs::create_dir_all(&self.config.rust_toolchain_dir)?;
            let rustup_init_path = self.config.downloads_cache.join("rustup-init");
            let arch_key = std::env::consts::ARCH.to_lowercase();
            let rustup_arch = if arch_key == "x86_64" || arch_key == "amd64" {
                "x86_64"
            } else if arch_key == "aarch64" || arch_key == "arm64" {
                "aarch64"
            } else {
                arch_key.as_str()
            };
            let rustup_url = format!(
                "https://static.rust-lang.org/rustup/dist/{rustup_arch}-unknown-linux-gnu/rustup-init"
            );

            let boot_result: Result<()> = (|| {
                fetch_url_to_file(&rustup_url, &rustup_init_path, None, 60)?;
                set_mode(&rustup_init_path, 0o755)?;

                let mut tc_env = build_env.clone();
                tc_env.insert(
                    "RUSTUP_HOME".to_string(),
                    self.config.rust_toolchain_dir.to_string_lossy().to_string(),
                );
                tc_env.insert(
                    "CARGO_HOME".to_string(),
                    self.config.rust_toolchain_dir.to_string_lossy().to_string(),
                );

                let cmd: Vec<String> = vec![
                    rustup_init_path.to_string_lossy().to_string(),
                    "-y".to_string(),
                    "--no-modify-path".to_string(),
                    "--profile".to_string(),
                    "minimal".to_string(),
                ];
                let res = self.executor.run(&cmd, None, &tc_env)?;
                if res.code != 0 {
                    return Err(AsterError::Runtime(format!(
                        "Failed to bootstrap isolated Rust toolchain: {}",
                        res.stderr
                    )));
                }
                Ok(())
            })();

            match boot_result {
                Ok(()) => {
                    cargo_bin = rust_toolchain_bin
                        .join("cargo")
                        .to_string_lossy()
                        .to_string();
                    using_isolated_toolchain = true;
                    downloaded_toolchain = true;
                }
                Err(e) => {
                    return Err(AsterError::Runtime(format!(
                        "Failed to install isolated Rust toolchain: {e}"
                    )));
                }
            }
        }

        if using_isolated_toolchain {
            build_env.insert(
                "RUSTUP_HOME".to_string(),
                self.config.rust_toolchain_dir.to_string_lossy().to_string(),
            );
            build_env.insert(
                "CARGO_HOME".to_string(),
                self.config.rust_toolchain_dir.to_string_lossy().to_string(),
            );
            let current = build_env.get("PATH").cloned().unwrap_or_default();
            build_env.insert(
                "PATH".to_string(),
                format!("{}:{}", rust_toolchain_bin.to_string_lossy(), current),
            );
        }

        let mut cargo_cmd: Vec<String> = vec![cargo_bin.clone()];
        if let Some(args) = custom_cargo_args.and_then(|v| v.as_array()) {
            for arg in args {
                cargo_cmd.push(arg.as_str().unwrap_or("").to_string());
            }
        } else {
            cargo_cmd.push("build".to_string());
            let release = build_info
                .map(|b| py_get_bool(b, "release", true))
                .unwrap_or(true);
            if release {
                cargo_cmd.push("--release".to_string());
            }
            let locked = build_info
                .map(|b| py_get_bool(b, "locked", false))
                .unwrap_or(false);
            if locked {
                cargo_cmd.push("--locked".to_string());
            }
        }

        let res = self.executor.run(&cargo_cmd, Some(build_dir), build_env)?;
        if res.code != 0 {
            if downloaded_toolchain {
                let cache_pref = self
                    .config
                    .load_json(&self.config.config_json)?
                    .get("cache_rust_toolchain")
                    .cloned();
                if cache_pref == Some(Value::Bool(false)) {
                    let _ = std::fs::remove_dir_all(&self.config.rust_toolchain_dir);
                }
            }
            return Err(self
                .handle_build_failure(&format!("{}\n{}", res.stdout, res.stderr), "Cargo build"));
        }

        let release = build_info
            .map(|b| py_get_bool(b, "release", true))
            .unwrap_or(true);
        let target_profile = if release { "release" } else { "debug" };
        let target_dir = build_dir.join("target").join(target_profile);

        let mut declared_execs: Vec<String> = Vec::new();
        if let Some(v) = pkg_def.get("executables") {
            declared_execs = value_to_string_list(v);
        }
        if declared_execs.is_empty() {
            if let Some(v) = build_info.and_then(|b| b.get("executables")) {
                declared_execs = value_to_string_list(v);
            }
        }

        let staging_bin = staging_dir.join("bin");
        std::fs::create_dir_all(&staging_bin)?;

        if !declared_execs.is_empty() {
            for exe_name in &declared_execs {
                let built_exe = target_dir.join(exe_name);
                if !built_exe.exists() {
                    return Err(AsterError::Runtime(format!(
                        "Declared executable '{exe_name}' was not produced at '{}'.",
                        built_exe.display()
                    )));
                }
                std::fs::copy(&built_exe, staging_bin.join(exe_name))?;
                set_mode(&staging_bin.join(exe_name), 0o755)?;
            }
        } else if target_dir.exists() {
            for item in read_dir_paths(&target_dir) {
                if item.is_file() && is_executable(&item) {
                    let fname = item
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if fname.starts_with('.') {
                        continue;
                    }
                    std::fs::copy(&item, staging_bin.join(&fname))?;
                    set_mode(&staging_bin.join(&fname), 0o755)?;
                }
            }
        }

        if downloaded_toolchain || rust_toolchain_bin.join("cargo").exists() {
            let mut cfg_data = self.config.load_json(&self.config.config_json)?;
            let cache_pref = cfg_data.get("cache_rust_toolchain").cloned();
            let mut effective = cache_pref.clone();

            if cache_pref.is_none() {
                println!("\nKeep the downloaded Rust toolchain and");
                println!("Cargo dependencies cached for future installs?");
                let choice = if auto_yes {
                    true
                } else {
                    match prompt_line("[Y/n]: ") {
                        Some(resp) => {
                            let resp = resp.to_lowercase();
                            resp.is_empty() || resp == "y" || resp == "yes"
                        }
                        None => true,
                    }
                };

                cfg_data.insert("cache_rust_toolchain".to_string(), Value::Bool(choice));
                self.config
                    .save_json_atomic(&self.config.config_json, &Value::Object(cfg_data))?;
                println!("\nPreference saved: cache_rust_toolchain = {choice}");
                println!("You can change this setting anytime using: aster config cache-rust <true|false>\n");
                effective = Some(Value::Bool(choice));
            }

            if effective == Some(Value::Bool(false)) && self.config.rust_toolchain_dir.exists() {
                let _ = std::fs::remove_dir_all(&self.config.rust_toolchain_dir);
            }
        }

        Ok(())
    }

    fn create_symlinks(
        &self,
        final_package_dir: &Path,
        provided_binaries: &[String],
    ) -> Result<()> {
        std::fs::create_dir_all(&self.config.bin_dir)?;
        for bin_name in provided_binaries {
            let mut target_bin = final_package_dir.join("bin").join(bin_name);
            if !target_bin.exists() {
                if let Some(found) = find_file(final_package_dir, bin_name) {
                    target_bin = found;
                }
            }

            if target_bin.exists() {
                set_mode(&target_bin, 0o755)?;
                let link_path = self.config.bin_dir.join(bin_name);
                if exists_or_symlink(&link_path) {
                    std::fs::remove_file(&link_path)?;
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target_bin, &link_path)?;
                #[cfg(not(unix))]
                std::fs::copy(&target_bin, &link_path)?;
            }
        }
        Ok(())
    }

    /// Removes an installed package.
    pub fn remove(&self, package_id: &str, force: bool) -> Result<()> {
        if !self.registry.is_installed(package_id)? {
            println!("Package '{package_id}' is not installed.");
            return Ok(());
        }

        if !force {
            let resolver = DependencyResolver::new(self.catalogue.clone(), self.registry.clone());
            let dependents = resolver.check_removal_safety(package_id)?;
            if !dependents.is_empty() {
                return Err(AsterError::Runtime(format!(
                    "Cannot remove '{package_id}': required by installed package(s): {}",
                    dependents.join(", ")
                )));
            }
        }

        let pkg_info = self.registry.get_installed_package(package_id)?;
        let provided_binaries: Vec<String> = pkg_info
            .as_ref()
            .and_then(|v| v.get("provided_binaries"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|b| b.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let pkg_dir = self.config.packages_dir.join(package_id);
        let pkg_resolved = std::fs::canonicalize(&pkg_dir).unwrap_or_else(|_| pkg_dir.clone());
        let pkg_resolved_str = pkg_resolved.to_string_lossy().to_string();

        for bin_name in &provided_binaries {
            let link_path = self.config.bin_dir.join(bin_name);
            if exists_or_symlink(&link_path) {
                match std::fs::canonicalize(&link_path) {
                    Ok(target) => {
                        if target.to_string_lossy().starts_with(&pkg_resolved_str)
                            || !link_path.exists()
                        {
                            std::fs::remove_file(&link_path)?;
                        }
                    }
                    Err(_) => {
                        if link_path.is_symlink() {
                            std::fs::remove_file(&link_path)?;
                        }
                    }
                }
            }
        }

        if pkg_dir.exists() {
            std::fs::remove_dir_all(&pkg_dir)?;
        }

        self.registry.unregister_package(package_id)?;
        println!("Successfully removed '{package_id}'.");
        Ok(())
    }
}

fn prompt_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim().to_string()),
        Err(_) => None,
    }
}

fn infer_archive_format(filename: &str) -> Option<String> {
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        Some("tar.gz".to_string())
    } else if filename.ends_with(".tar.xz") || filename.ends_with(".txz") {
        Some("tar.xz".to_string())
    } else if filename.ends_with(".zip") {
        Some("zip".to_string())
    } else {
        None
    }
}

fn read_dir_paths(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
    }
    walk(root, &mut out);
    out
}

fn detect_compression(file: std::fs::File) -> Result<Box<dyn Read>> {
    let mut reader = BufReader::new(file);
    let magic = reader.fill_buf()?;
    if magic.starts_with(&[0x1f, 0x8b]) {
        Ok(Box::new(GzDecoder::new(reader)))
    } else if magic.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        Ok(Box::new(XzDecoder::new(reader)))
    } else {
        Ok(Box::new(reader))
    }
}

fn extract_tar(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let reader = detect_compression(file)?;
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let name = path.to_string_lossy().to_string();
        if name.starts_with('/') || name.contains("..") {
            return Err(AsterError::Runtime(format!(
                "Unsafe file path in archive: {name}"
            )));
        }
        entry.unpack_in(dest)?;
    }
    Ok(())
}

fn extract_zip(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| AsterError::Other(e.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AsterError::Other(e.to_string()))?;
        let name = entry.name().to_string();
        if name.starts_with('/') || name.contains("..") {
            return Err(AsterError::Runtime(format!(
                "Unsafe file path in archive: {name}"
            )));
        }
        let outpath = dest.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&outpath)?;
            std::io::copy(&mut entry, &mut out)?;
            drop(out);
            if let Some(mode) = entry.unix_mode() {
                if mode & 0o111 != 0 {
                    set_mode(&outpath, mode & 0o777)?;
                }
            }
        }
    }
    Ok(())
}

fn unnest_single_dir(dir: &Path) -> Result<()> {
    let items = read_dir_paths(dir);
    if items.len() == 1 && items[0].is_dir() {
        let nested = &items[0];
        let tmp_nested = dir.join("__nested_tmp__");
        std::fs::rename(nested, &tmp_nested)?;
        for entry in std::fs::read_dir(&tmp_nested)?.flatten() {
            std::fs::rename(entry.path(), dir.join(entry.file_name()))?;
        }
        std::fs::remove_dir_all(&tmp_nested)?;
    }
    Ok(())
}

fn unnest_build_dir(build_dir: &Path) -> Result<()> {
    let items: Vec<PathBuf> = read_dir_paths(build_dir)
        .into_iter()
        .filter(|p| p.file_name().map(|n| n != ".git").unwrap_or(true))
        .collect();
    if items.len() == 1
        && items[0].is_dir()
        && !build_dir.join("CMakeLists.txt").exists()
        && !build_dir.join("Makefile").exists()
    {
        let nested = &items[0];
        let tmp_nested = build_dir.join("__nested_tmp__");
        std::fs::rename(nested, &tmp_nested)?;
        for entry in std::fs::read_dir(&tmp_nested)?.flatten() {
            std::fs::rename(entry.path(), build_dir.join(entry.file_name()))?;
        }
        std::fs::remove_dir_all(&tmp_nested)?;
    }
    Ok(())
}

fn value_to_string_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}
