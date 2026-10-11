#![allow(dead_code)]

use aster::catalogue::CatalogueManager;
use aster::config::AsterConfig;
use aster::installer::{CommandExecutor, CommandOutput, PackageInstaller};
use aster::registry::RegistryManager;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

pub fn make_tar_gz(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let file = fs::File::create(path).unwrap();
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    write_tar(enc, entries);
}

pub fn make_tar_xz(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let file = fs::File::create(path).unwrap();
    let enc = xz2::write::XzEncoder::new(file, 6);
    write_tar(enc, entries);
}

fn write_tar<W: Write>(writer: W, entries: &[(&str, &[u8], u32)]) {
    let mut builder = tar::Builder::new(writer);
    for &(name, data, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
    }
    builder.finish().unwrap();
}

pub fn make_zip(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for &(name, data, mode) in entries {
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(mode);
        zip.start_file(name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

pub fn makefile_content(bin: &str) -> String {
    format!(
        "all:\n\t@echo 'build complete'\ninstall:\n\tmkdir -p $(DESTDIR)/bin\n\techo '#!/bin/sh' > $(DESTDIR)/bin/{bin}\n\tchmod +x $(DESTDIR)/bin/{bin}\n"
    )
}

pub struct Fixture {
    pub temp: TempDir,
    pub root: PathBuf,
    pub cat_dir: PathBuf,
    pub config: AsterConfig,
}

impl Fixture {
    pub fn new() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let tmp = temp.path();
        let root = tmp.join("aster");
        let cat_dir = tmp.join("catalogue");
        fs::create_dir_all(cat_dir.join("packages")).unwrap();

        let bin_asset_dir = tmp.join("assets");
        fs::create_dir_all(&bin_asset_dir).unwrap();

        // fastfetch binary archive
        let fastfetch_archive = bin_asset_dir.join("fastfetch-bin.tar.gz");
        make_tar_gz(
            &fastfetch_archive,
            &[("bin/fastfetch", b"#!/bin/sh\necho fastfetch", 0o755)],
        );
        write_json(
            &cat_dir.join("packages/fastfetch-bin.json"),
            &json!({
                "schema_version": 1,
                "id": "fastfetch-bin",
                "name": "Fastfetch",
                "version": "2.30.0",
                "type": "binary",
                "description": "Precompiled fastfetch",
                "downloads": {
                    "linux-x86_64": {"url": format!("file://{}", fastfetch_archive.display()), "sha256": "EXPECTED_SHA256"},
                    "linux-aarch64": {"url": format!("file://{}", fastfetch_archive.display()), "sha256": "EXPECTED_SHA256"}
                },
                "supported_platforms": ["linux-x86_64", "linux-aarch64"]
            }),
        );

        // source asset
        let src_asset_dir = tmp.join("src_asset");
        fs::create_dir_all(&src_asset_dir).unwrap();
        fs::write(src_asset_dir.join("Makefile"), makefile_content("test-src")).unwrap();
        let src_archive = tmp.join("test-src.tar.gz");
        make_tar_gz(
            &src_archive,
            &[("Makefile", makefile_content("test-src").as_bytes(), 0o644)],
        );
        write_json(
            &cat_dir.join("packages/test-src.json"),
            &json!({
                "schema_version": 1,
                "id": "test-src",
                "name": "Test Source",
                "version": "1.0.0",
                "type": "source",
                "description": "Test source package",
                "source": {"type": "tar.gz", "url": format!("file://{}", src_archive.display())},
                "build": {"system": "make"}
            }),
        );

        // ripgrep nested binary archive
        let ripgrep_archive = tmp.join("ripgrep-14.1.0-x86_64-linux.tar.gz");
        make_tar_gz(
            &ripgrep_archive,
            &[("ripgrep-14.1.0-x86_64/rg", b"#!/bin/sh\necho rg", 0o755)],
        );
        write_json(
            &cat_dir.join("packages/ripgrep-bin.json"),
            &json!({
                "schema_version": 1,
                "id": "ripgrep-bin",
                "name": "ripgrep",
                "version": "14.1.0",
                "type": "binary",
                "description": "line-oriented search tool",
                "downloads": {
                    "linux-x86_64": {"url": format!("file://{}", ripgrep_archive.display())},
                    "linux-aarch64": {"url": format!("file://{}", ripgrep_archive.display())}
                }
            }),
        );

        // htop custom steps source package
        write_json(
            &cat_dir.join("packages/htop-src.json"),
            &json!({
                "schema_version": 1,
                "id": "htop-src",
                "name": "htop",
                "version": "3.5.3",
                "type": "source",
                "description": "Interactive process viewer",
                "source": {"type": "tar.gz", "url": format!("file://{}", src_archive.display())},
                "build": {
                    "steps": [
                        "mkdir -p staging_bin",
                        "echo '#!/bin/sh' > staging_bin/htop",
                        "chmod +x staging_bin/htop"
                    ]
                }
            }),
        );

        // dependency packages
        let dep_lib_archive = bin_asset_dir.join("dep-lib.tar.gz");
        make_tar_gz(
            &dep_lib_archive,
            &[("bin/dep-lib", b"#!/bin/sh\necho dep-lib", 0o755)],
        );
        write_json(
            &cat_dir.join("packages/dep-lib.json"),
            &json!({
                "schema_version": 1,
                "id": "dep-lib",
                "name": "DepLib",
                "version": "1.0.0",
                "type": "binary",
                "downloads": {
                    "linux-x86_64": {"url": format!("file://{}", dep_lib_archive.display())},
                    "linux-aarch64": {"url": format!("file://{}", dep_lib_archive.display())}
                }
            }),
        );

        let app_dep_archive = bin_asset_dir.join("app-dep.tar.gz");
        make_tar_gz(
            &app_dep_archive,
            &[("bin/app-dep", b"#!/bin/sh\necho app", 0o755)],
        );
        write_json(
            &cat_dir.join("packages/app-with-dep.json"),
            &json!({
                "schema_version": 1,
                "id": "app-with-dep",
                "name": "AppWithDep",
                "version": "1.0.0",
                "type": "binary",
                "dependencies": ["dep-lib"],
                "downloads": {
                    "linux-x86_64": {"url": format!("file://{}", app_dep_archive.display())},
                    "linux-aarch64": {"url": format!("file://{}", app_dep_archive.display())}
                }
            }),
        );

        let index_data = json!({
            "schema_version": 1,
            "packages": {
                "fastfetch-bin": {"definition": "packages/fastfetch-bin.json", "type": "binary", "description": "Precompiled fastfetch"},
                "test-src": {"definition": "packages/test-src.json", "type": "source", "description": "Test source package"},
                "dep-lib": {"definition": "packages/dep-lib.json", "type": "binary", "description": "Dependency library"},
                "app-with-dep": {"definition": "packages/app-with-dep.json", "type": "binary", "description": "App requiring dep-lib"},
                "ripgrep-bin": {"definition": "packages/ripgrep-bin.json", "type": "binary", "description": "line-oriented search tool"},
                "htop-src": {"definition": "packages/htop-src.json", "type": "source", "description": "Interactive process viewer"}
            }
        });
        write_json(&cat_dir.join("index.json"), &index_data);

        let config = AsterConfig::with_root_dir(root.clone());
        config.ensure_directories().unwrap();
        let repos = json!({
            "schema_version": 1,
            "repositories": {
                "default": {"name": "default", "url": cat_dir.to_string_lossy()}
            }
        });
        config
            .save_json_atomic(&config.repositories_json, &repos)
            .unwrap();

        Fixture {
            temp,
            root,
            cat_dir,
            config,
        }
    }

    pub fn registry(&self) -> RegistryManager {
        RegistryManager::new(self.config.clone()).unwrap()
    }

    pub fn catalogue(&self) -> CatalogueManager {
        CatalogueManager::new(self.config.clone()).unwrap()
    }

    pub fn installer(&self) -> PackageInstaller {
        PackageInstaller::new(self.config.clone(), self.registry(), self.catalogue())
    }

    pub fn installer_with(&self, executor: Arc<dyn CommandExecutor>) -> PackageInstaller {
        PackageInstaller::with_executor(
            self.config.clone(),
            self.registry(),
            self.catalogue(),
            executor,
        )
    }

    pub fn run_cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aster"))
            .args(args)
            .env("ASTER_HOME", &self.root)
            .output()
            .unwrap()
    }
}

pub fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string(value).unwrap()).unwrap();
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

type RunHook = Box<dyn Fn(&[String], Option<&Path>) + Send + Sync>;

pub struct FakeExecutor {
    pub commands: Mutex<Vec<Vec<String>>>,
    pub envs: Mutex<Vec<HashMap<String, String>>>,
    which_map: Mutex<HashMap<String, String>>,
    on_run: RunHook,
}

impl FakeExecutor {
    pub fn new() -> Self {
        FakeExecutor {
            commands: Mutex::new(Vec::new()),
            envs: Mutex::new(Vec::new()),
            which_map: Mutex::new(HashMap::new()),
            on_run: Box::new(|_, _| {}),
        }
    }

    pub fn with_which(self, name: &str, path: &str) -> Self {
        self.which_map
            .lock()
            .unwrap()
            .insert(name.to_string(), path.to_string());
        self
    }

    pub fn on_run<F: Fn(&[String], Option<&Path>) + Send + Sync + 'static>(mut self, f: F) -> Self {
        self.on_run = Box::new(f);
        self
    }

    pub fn commands(&self) -> Vec<Vec<String>> {
        self.commands.lock().unwrap().clone()
    }

    pub fn envs(&self) -> Vec<HashMap<String, String>> {
        self.envs.lock().unwrap().clone()
    }
}

impl CommandExecutor for FakeExecutor {
    fn run(
        &self,
        argv: &[String],
        cwd: Option<&Path>,
        env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput> {
        self.commands.lock().unwrap().push(argv.to_vec());
        self.envs.lock().unwrap().push(env.clone());
        (self.on_run)(argv, cwd);
        Ok(CommandOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn run_shell(
        &self,
        script: &str,
        cwd: Option<&Path>,
        _env: &HashMap<String, String>,
    ) -> std::io::Result<CommandOutput> {
        let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string()];
        self.commands.lock().unwrap().push(argv.clone());
        (self.on_run)(&argv, cwd);
        Ok(CommandOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn which(&self, name: &str, _path: Option<&str>) -> Option<PathBuf> {
        self.which_map.lock().unwrap().get(name).map(PathBuf::from)
    }
}
