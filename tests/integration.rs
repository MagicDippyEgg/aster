mod common;

use aster::installer::PackageInstaller;
use aster::resolver::DependencyResolver;
use aster::schema::{validate_index, validate_package_definition, validate_registry};
use aster::version::compare_versions;
use common::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[test]
fn test_schema_validation() {
    assert!(validate_package_definition(&json!({})).is_err());
    assert!(validate_package_definition(&json!({
        "schema_version": 1, "id": "x", "name": "x", "version": "1", "type": "invalid"
    }))
    .is_err());
    validate_package_definition(&json!({
        "schema_version": 1, "id": "x", "name": "x", "version": "1", "type": "binary"
    }))
    .unwrap();

    assert!(validate_index(&json!({})).is_err());
    validate_index(&json!({"packages": {}})).unwrap();
    assert!(validate_registry(&json!({})).is_err());
    validate_registry(&json!({"installed": {}})).unwrap();
}

#[test]
fn test_version_comparison() {
    assert_eq!(compare_versions("1.0.0", "1.0.1"), -1);
    assert_eq!(compare_versions("2.0.0", "1.9.9"), 1);
    assert_eq!(compare_versions("1.0.0", "1.0.0"), 0);
    assert_eq!(compare_versions("1.0.0-rc1", "1.0.0"), -1);
}

#[test]
fn test_config_and_registry() {
    let f = Fixture::new();
    let registry = f.registry();
    assert!(!registry.is_installed("fastfetch-bin").unwrap());
    assert!(registry.list_installed().unwrap().is_empty());
}

#[test]
fn test_catalogue_search_and_update() {
    let f = Fixture::new();
    let catalogue = f.catalogue();
    catalogue.update_all().unwrap();

    let results = catalogue.search_packages("fastfetch").unwrap();
    assert!(results.contains_key("fastfetch-bin"));

    let pkg_def = catalogue
        .get_package_definition("fastfetch-bin", None)
        .unwrap()
        .unwrap();
    assert_eq!(pkg_def["id"], "fastfetch-bin");
}

#[test]
fn test_dependency_resolution() {
    let f = Fixture::new();
    let catalogue = f.catalogue();
    let registry = f.registry();
    catalogue.update_all().unwrap();

    let resolver = DependencyResolver::new(catalogue, registry);
    let plan = resolver.resolve_dependencies("app-with-dep").unwrap();
    assert_eq!(
        plan,
        vec!["dep-lib".to_string(), "app-with-dep".to_string()]
    );
}

#[test]
fn test_cli_full_workflow() {
    let f = Fixture::new();

    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);

    let out = f.run_cli(&["search", "fastfetch"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("fastfetch-bin"));

    assert_eq!(code(&f.run_cli(&["installed", "fastfetch-bin"])), 1);
    assert_eq!(code(&f.run_cli(&["install", "fastfetch-bin"])), 0);
    assert_eq!(code(&f.run_cli(&["installed", "fastfetch-bin"])), 0);

    let out = f.run_cli(&["list"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("fastfetch-bin"));

    assert_eq!(code(&f.run_cli(&["info", "fastfetch-bin"])), 0);

    let bin_link = f.config.bin_dir.join("fastfetch");
    assert!(bin_link.exists());

    assert_eq!(code(&f.run_cli(&["doctor"])), 0);
    assert_eq!(code(&f.run_cli(&["clean"])), 0);

    assert_eq!(code(&f.run_cli(&["remove", "fastfetch-bin"])), 0);
    assert!(!bin_link.exists());
    assert_eq!(code(&f.run_cli(&["installed", "fastfetch-bin"])), 1);
}

#[test]
fn test_install_with_dependencies() {
    let f = Fixture::new();
    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);
    assert_eq!(code(&f.run_cli(&["install", "app-with-dep"])), 0);

    assert!(f.config.bin_dir.join("dep-lib").exists());
    assert!(f.config.bin_dir.join("app-dep").exists());

    assert_eq!(code(&f.run_cli(&["remove", "dep-lib"])), 1);
    assert_eq!(code(&f.run_cli(&["remove", "dep-lib", "--force"])), 0);
}

#[test]
fn test_install_custom_steps_package() {
    let f = Fixture::new();
    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);
    assert_eq!(code(&f.run_cli(&["install", "htop-src", "-y"])), 0);
    assert!(f.config.bin_dir.join("htop").exists());
    assert_eq!(code(&f.run_cli(&["remove", "htop-src"])), 0);
}

#[test]
fn test_install_nested_binary_package() {
    let f = Fixture::new();
    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);
    assert_eq!(code(&f.run_cli(&["install", "ripgrep-bin"])), 0);
    assert!(f.config.bin_dir.join("rg").exists());
    assert_eq!(code(&f.run_cli(&["remove", "ripgrep-bin"])), 0);
}

#[test]
fn test_missing_declared_executable_validation() {
    let f = Fixture::new();
    let dummy_tar = f.temp.path().join("dummy.tar.gz");
    make_tar_gz(&dummy_tar, &[]);

    let exec = FakeExecutor::new().with_which("cargo", "/usr/bin/cargo");
    let installer = f.installer_with(Arc::new(exec));

    let fd_def = json!({
        "schema_version": 1,
        "id": "fd-src",
        "name": "fd",
        "version": "10.0.0",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", dummy_tar.display())},
        "executables": ["fd"],
        "build": {"system": "cargo"}
    });

    let err = installer.install_source(&fd_def, true).unwrap_err();
    assert!(err
        .to_string()
        .contains("Declared executable 'fd' was not produced"));
}

#[test]
fn test_cargo_detection_system_vs_isolated() {
    let f = Fixture::new();
    let dummy_tar = f.temp.path().join("dummy2.tar.gz");
    make_tar_gz(&dummy_tar, &[]);

    let exec = FakeExecutor::new()
        .with_which("cargo", "/usr/bin/cargo")
        .on_run(|argv, cwd| {
            if argv[0].contains("cargo") {
                if let Some(cwd) = cwd {
                    let target = cwd.join("target").join("release");
                    fs::create_dir_all(&target).unwrap();
                    let fd = target.join("fd");
                    fs::write(&fd, "#!/bin/sh\necho fd").unwrap();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&fd, fs::Permissions::from_mode(0o755)).unwrap();
                    }
                }
            }
        });
    let exec = Arc::new(exec);
    let installer = f.installer_with(exec.clone());

    let fd_def = json!({
        "schema_version": 1,
        "id": "fd-src",
        "name": "fd",
        "version": "10.0.0",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", dummy_tar.display())},
        "executables": ["fd"],
        "build": {"system": "cargo"}
    });

    installer.install_source(&fd_def, true).unwrap();

    let envs = exec.envs();
    let build_env = &envs[0];
    let toolchain = f.config.rust_toolchain_dir.to_string_lossy().to_string();
    assert!(build_env
        .get("RUSTUP_HOME")
        .map(|v| v != &toolchain)
        .unwrap_or(true));
    assert!(build_env
        .get("CARGO_HOME")
        .map(|v| v != &toolchain)
        .unwrap_or(true));
}

#[test]
fn test_install_flattening_name_collision() {
    let f = Fixture::new();
    let installer = f.installer();
    let registry = f.registry();

    let age_archive = f.temp.path().join("age-v1.1.1-linux-amd64.tar.gz");
    make_tar_gz(&age_archive, &[("age/age", b"#!/bin/sh\necho age", 0o755)]);

    let age_bin_def = json!({
        "schema_version": 1,
        "id": "age-bin",
        "name": "age",
        "version": "1.1.1",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": format!("file://{}", age_archive.display())},
            "linux-aarch64": {"url": format!("file://{}", age_archive.display())}
        }
    });

    installer.install_binary(&age_bin_def).unwrap();
    assert!(registry.is_installed("age-bin").unwrap());
    assert!(f.config.bin_dir.join("age").exists());
}

#[test]
fn test_install_tar_xz_binary_and_source() {
    let f = Fixture::new();
    let installer = f.installer();
    let registry = f.registry();

    let bin_tar_xz = f.temp.path().join("xzbin-1.0.0.tar.xz");
    make_tar_xz(
        &bin_tar_xz,
        &[("bin/xzbin", b"#!/bin/sh\necho xzbin", 0o755)],
    );
    let bin_pkg_def = json!({
        "schema_version": 1,
        "id": "xzbin-bin",
        "name": "XZBin",
        "version": "1.0.0",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": format!("file://{}", bin_tar_xz.display())},
            "linux-aarch64": {"url": format!("file://{}", bin_tar_xz.display())}
        }
    });
    installer.install_binary(&bin_pkg_def).unwrap();
    assert!(registry.is_installed("xzbin-bin").unwrap());
    assert!(f.config.bin_dir.join("xzbin").exists());

    let src_tar_xz = f.temp.path().join("xzsrc-1.0.0.tar.xz");
    make_tar_xz(
        &src_tar_xz,
        &[("Makefile", makefile_content("xzsrc").as_bytes(), 0o644)],
    );
    let src_pkg_def = json!({
        "schema_version": 1,
        "id": "xzsrc-src",
        "name": "XZSrc",
        "version": "1.0.0",
        "type": "source",
        "source": {"type": "tar.xz", "url": format!("file://{}", src_tar_xz.display())},
        "build": {"system": "make"}
    });
    installer.install_source(&src_pkg_def, true).unwrap();
    assert!(registry.is_installed("xzsrc-src").unwrap());
    assert!(f.config.bin_dir.join("xzsrc").exists());
}

#[test]
fn test_install_zip_preserves_executable_bits_and_discovers_multiple_binaries() {
    let f = Fixture::new();
    let installer = f.installer();
    let registry = f.registry();

    let archive_path = f.temp.path().join("yazi-release.zip");
    make_zip(
        &archive_path,
        &[
            ("yazi-release/yazi", b"#!/bin/sh\necho yazi\n", 0o755),
            ("yazi-release/ya", b"#!/bin/sh\necho ya\n", 0o755),
        ],
    );

    let package = json!({
        "schema_version": 1,
        "id": "yazi-bin",
        "name": "yazi",
        "version": "26.9.1",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": format!("file://{}", archive_path.display())},
            "linux-aarch64": {"url": format!("file://{}", archive_path.display())}
        }
    });

    installer.install_binary(&package).unwrap();

    assert!(f.config.bin_dir.join("yazi").is_symlink());
    assert!(f.config.bin_dir.join("ya").is_symlink());
    let installed = registry.get_installed_package("yazi-bin").unwrap().unwrap();
    let mut provided: Vec<String> = installed["provided_binaries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    provided.sort();
    assert_eq!(provided, vec!["ya".to_string(), "yazi".to_string()]);
}

#[test]
fn test_cargo_custom_args_builds_declared_workspace_executables() {
    let f = Fixture::new();

    let source_dir = f.temp.path().join("yazi-source");
    fs::create_dir_all(&source_dir).unwrap();
    let source_archive = f.temp.path().join("yazi-source.tar.gz");
    make_tar_gz(
        &source_archive,
        &[("Cargo.toml", b"[workspace]\nmembers = []\n", 0o644)],
    );

    let exec = FakeExecutor::new()
        .with_which("cargo", "/usr/bin/cargo")
        .on_run(|argv, cwd| {
            if argv[0].contains("cargo") {
                if let Some(cwd) = cwd {
                    let target = cwd.join("target").join("release");
                    fs::create_dir_all(&target).unwrap();
                    for name in ["yazi", "ya"] {
                        let exe = target.join(name);
                        fs::write(&exe, format!("#!/bin/sh\necho {name}\n")).unwrap();
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
                        }
                    }
                }
            }
        });
    let exec = Arc::new(exec);
    let installer = f.installer_with(exec.clone());

    let package = json!({
        "schema_version": 1,
        "id": "yazi-src",
        "name": "yazi",
        "version": "26.9.1",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", source_archive.display())},
        "build": {"system": "cargo", "cargo_args": ["xtask", "build"]},
        "executables": ["yazi", "ya"]
    });

    installer.install_source(&package, true).unwrap();

    let commands = exec.commands();
    assert_eq!(
        commands,
        vec![vec![
            "/usr/bin/cargo".to_string(),
            "xtask".to_string(),
            "build".to_string()
        ]]
    );
    assert!(f.config.bin_dir.join("yazi").is_symlink());
    assert!(f.config.bin_dir.join("ya").is_symlink());
}

#[test]
fn test_cleanup_on_failure() {
    let f = Fixture::new();
    let installer = f.installer();

    let failing_def = json!({
        "schema_version": 1,
        "id": "failing-pkg",
        "name": "Failing Package",
        "version": "1.0.0",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": "file:///nonexistent/archive.tar.gz"}
        }
    });

    let err = installer.install_binary(&failing_def).unwrap_err();
    assert!(matches!(err, aster::error::AsterError::NotFound(_)));

    assert!(!f.config.build_dir.join("staging-failing-pkg").exists());
    assert!(!f.config.build_dir.join("build-failing-pkg").exists());
    assert!(!f.config.packages_dir.join("failing-pkg").exists());
}

#[test]
fn test_repository_priority_resolution() {
    let f = Fixture::new();

    let repo_a_dir = f.temp.path().join("repo_a");
    fs::create_dir_all(repo_a_dir.join("packages")).unwrap();
    write_json(
        &repo_a_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "common-app": {"definition": "packages/common-app.json", "type": "binary", "description": "Common App from Repo A (low priority)"}
            }
        }),
    );
    write_json(
        &repo_a_dir.join("packages/common-app.json"),
        &json!({"schema_version": 1, "id": "common-app", "name": "CommonAppA", "version": "1.0.0", "type": "binary"}),
    );

    let repo_b_dir = f.temp.path().join("repo_b");
    fs::create_dir_all(repo_b_dir.join("packages")).unwrap();
    write_json(
        &repo_b_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "common-app": {"definition": "packages/common-app.json", "type": "binary", "description": "Common App from Repo B (high priority)"}
            }
        }),
    );
    write_json(
        &repo_b_dir.join("packages/common-app.json"),
        &json!({"schema_version": 1, "id": "common-app", "name": "CommonAppB", "version": "2.0.0", "type": "binary"}),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "repo-a": {"name": "repo-a", "url": repo_a_dir.to_string_lossy(), "priority": 50},
            "repo-b": {"name": "repo-b", "url": repo_b_dir.to_string_lossy(), "priority": 200}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    let catalogue = f.catalogue();
    catalogue.update_all().unwrap();

    let sorted = catalogue.get_sorted_repositories().unwrap();
    let names: Vec<String> = sorted.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(names, vec!["repo-b".to_string(), "repo-a".to_string()]);

    let search_res = catalogue.search_packages("common-app").unwrap();
    assert_eq!(
        search_res["common-app"]["description"],
        "Common App from Repo B (high priority)"
    );
    assert_eq!(search_res["common-app"]["repository"], "repo-b");

    let pkg_def = catalogue
        .get_package_definition("common-app", None)
        .unwrap()
        .unwrap();
    assert_eq!(pkg_def["name"], "CommonAppB");
    assert_eq!(pkg_def["version"], "2.0.0");
}

#[test]
fn test_cli_repo_priority_commands() {
    let f = Fixture::new();

    assert_eq!(
        code(&f.run_cli(&[
            "repo",
            "add",
            "custom-repo",
            "http://example.com/repo",
            "-p",
            "150"
        ])),
        0
    );
    let repos_data = f.config.load_json(&f.config.repositories_json).unwrap();
    assert_eq!(repos_data["repositories"]["custom-repo"]["priority"], 150);

    let out = f.run_cli(&["repo", "list"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("custom-repo"));
    assert!(stdout(&out).contains("150"));

    assert_eq!(
        code(&f.run_cli(&["repo", "set-priority", "custom-repo", "300"])),
        0
    );
    let repos_data = f.config.load_json(&f.config.repositories_json).unwrap();
    assert_eq!(repos_data["repositories"]["custom-repo"]["priority"], 300);

    assert_eq!(
        code(&f.run_cli(&["repo", "set-priority", "nonexistent-repo", "200"])),
        1
    );
}

#[test]
fn test_package_installer_public_api() {
    // Guard against accidental signature drift for the test seam.
    let _ctor: fn(
        aster::config::AsterConfig,
        aster::registry::RegistryManager,
        aster::catalogue::CatalogueManager,
    ) -> PackageInstaller = PackageInstaller::new;
}

#[test]
fn test_cache_rust_invalid_value_does_not_modify_config() {
    let f = Fixture::new();

    fs::create_dir_all(&f.config.rust_toolchain_dir).unwrap();
    let marker = f.config.rust_toolchain_dir.join("marker");
    fs::write(&marker, b"keep").unwrap();

    let before = fs::read_to_string(&f.config.config_json).unwrap();

    let out = f.run_cli(&["config", "cache-rust", "ture"]);
    assert_ne!(code(&out), 0, "invalid value must be rejected");
    assert!(!stdout(&out).contains("Set 'cache_rust_toolchain'"));
    let after = fs::read_to_string(&f.config.config_json).unwrap();
    assert_eq!(before, after, "invalid input must not modify the config");
    assert!(
        marker.exists(),
        "invalid input must not delete the cached Rust toolchain"
    );

    // Valid values are still accepted.
    assert_eq!(code(&f.run_cli(&["config", "cache-rust", "no"])), 0);
    let cfg = f.config.load_json(&f.config.config_json).unwrap();
    assert_eq!(cfg.get("cache_rust_toolchain"), Some(&json!(false)));
    assert!(
        !f.config.rust_toolchain_dir.exists(),
        "disabling caching should clean the cached toolchain"
    );

    assert_eq!(code(&f.run_cli(&["config", "cache-rust", "yes"])), 0);
    let cfg = f.config.load_json(&f.config.config_json).unwrap();
    assert_eq!(cfg.get("cache_rust_toolchain"), Some(&json!(true)));
}

#[test]
fn test_empty_build_steps_falls_through_to_cargo() {
    let f = Fixture::new();
    let archive = f.temp.path().join("empty-steps-src.tar.gz");
    make_tar_gz(
        &archive,
        &[("Cargo.toml", b"[package]\nname = \"emptysteps\"\n", 0o644)],
    );

    let exec = FakeExecutor::new()
        .with_which("cargo", "/usr/bin/cargo")
        .on_run(|argv, cwd| {
            if argv[0].contains("cargo") {
                if let Some(cwd) = cwd {
                    let target = cwd.join("target").join("release");
                    fs::create_dir_all(&target).unwrap();
                    let exe = target.join("emptysteps");
                    fs::write(&exe, "#!/bin/sh\necho ok\n").unwrap();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
                    }
                }
            }
        });
    let exec = Arc::new(exec);
    let installer = f.installer_with(exec.clone());

    let def = json!({
        "schema_version": 1,
        "id": "emptysteps-src",
        "name": "emptysteps",
        "version": "1.0.0",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", archive.display())},
        "build": {"system": "cargo", "steps": []},
        "executables": ["emptysteps"]
    });

    installer.install_source(&def, true).unwrap();

    let commands = exec.commands();
    assert!(
        commands
            .iter()
            .any(|c| c.first().map(|s| s.contains("cargo")).unwrap_or(false)),
        "cargo build should run when steps is an empty array"
    );
    assert!(f.config.bin_dir.join("emptysteps").exists());
}

#[test]
fn test_search_uses_winning_repository_definition() {
    let f = Fixture::new();

    let repo_high = f.temp.path().join("repo_high");
    fs::create_dir_all(repo_high.join("packages")).unwrap();
    write_json(
        &repo_high.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "toolkit": {"definition": "packages/toolkit.json", "type": "binary", "description": "High priority build"}
            }
        }),
    );
    write_json(
        &repo_high.join("packages/toolkit.json"),
        &json!({"schema_version": 1, "id": "toolkit", "name": "ToolkitHigh", "version": "2.0.0", "type": "binary"}),
    );

    let repo_low = f.temp.path().join("repo_low");
    fs::create_dir_all(repo_low.join("packages")).unwrap();
    write_json(
        &repo_low.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "toolkit": {"definition": "packages/toolkit.json", "type": "binary", "description": "alpha searchable text"}
            }
        }),
    );
    write_json(
        &repo_low.join("packages/toolkit.json"),
        &json!({"schema_version": 1, "id": "toolkit", "name": "ToolkitLow", "version": "1.0.0", "type": "binary"}),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "repo-high": {"name": "repo-high", "url": repo_high.to_string_lossy(), "priority": 200},
            "repo-low": {"name": "repo-low", "url": repo_low.to_string_lossy(), "priority": 50}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    let catalogue = f.catalogue();
    catalogue.update_all().unwrap();

    // A term matching only the lower-priority copy must not surface it,
    // because installation would select the higher-priority definition.
    let results = catalogue.search_packages("alpha").unwrap();
    assert!(
        !results.contains_key("toolkit"),
        "search must not show a lower-priority copy that installation would not select"
    );

    // When the query matches the id, search reports the winning repository.
    let by_id = catalogue.search_packages("toolkit").unwrap();
    assert_eq!(by_id["toolkit"]["repository"], "repo-high");
    assert_eq!(by_id["toolkit"]["description"], "High priority build");

    // Installation resolves the same definition.
    let def = catalogue
        .get_package_definition("toolkit", None)
        .unwrap()
        .unwrap();
    assert_eq!(def["name"], "ToolkitHigh");
}

#[test]
fn test_install_records_actual_source_repository() {
    let f = Fixture::new();

    let community = f.temp.path().join("community");
    fs::create_dir_all(community.join("packages")).unwrap();
    let archive = f.temp.path().join("comm-pkg.tar.gz");
    make_tar_gz(
        &archive,
        &[("bin/comm-pkg", b"#!/bin/sh\necho comm", 0o755)],
    );
    write_json(
        &community.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "comm-pkg": {"definition": "packages/comm-pkg.json", "type": "binary", "description": "Community package"}
            }
        }),
    );
    write_json(
        &community.join("packages/comm-pkg.json"),
        &json!({
            "schema_version": 1,
            "id": "comm-pkg",
            "name": "CommPkg",
            "version": "1.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": format!("file://{}", archive.display())},
                "linux-aarch64": {"url": format!("file://{}", archive.display())}
            }
        }),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": f.cat_dir.to_string_lossy(), "priority": 100},
            "community": {"name": "community", "url": community.to_string_lossy(), "priority": 200}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();

    f.installer().install("comm-pkg", true).unwrap();

    let info = f
        .registry()
        .get_installed_package("comm-pkg")
        .unwrap()
        .unwrap();
    assert_eq!(info["source_repository"], "community");

    let out = f.run_cli(&["list"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("community"));
}

fn provided_binaries(f: &Fixture, pkg_id: &str) -> Vec<String> {
    let info = f.registry().get_installed_package(pkg_id).unwrap().unwrap();
    info["provided_binaries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn test_source_discovery_ignores_git_hook_samples() {
    let f = Fixture::new();
    let archive = f.temp.path().join("tree-src.tar.gz");
    make_tar_gz(
        &archive,
        &[
            (".git/hooks/applypatch-msg.sample", b"#!/bin/sh\n:\n", 0o755),
            (".git/hooks/pre-commit.sample", b"#!/bin/sh\n:\n", 0o755),
            ("bin/example-command", b"#!/bin/sh\necho example\n", 0o755),
            ("Makefile", b"all:\n\t@true\ninstall:\n\t@true\n", 0o644),
        ],
    );

    // make/make install are no-ops, so staging stays empty and the source-tree
    // fallback discovery path runs.
    let installer = f.installer_with(Arc::new(FakeExecutor::new()));

    let def = json!({
        "schema_version": 1,
        "id": "example-src",
        "name": "example",
        "version": "1.0.0",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", archive.display())},
        "build": {"system": "make"}
    });

    installer.install_source(&def, true).unwrap();

    let provided = provided_binaries(&f, "example-src");
    assert!(
        provided.contains(&"example-command".to_string()),
        "real command should be discovered: {provided:?}"
    );
    assert!(
        !provided
            .iter()
            .any(|n| n.contains("applypatch") || n.contains("pre-commit") || n.contains("sample")),
        "git hook scripts must not be discovered: {provided:?}"
    );
    assert!(f.config.bin_dir.join("example-command").exists());
    assert!(!f.config.bin_dir.join("applypatch-msg.sample").exists());
    assert!(!f.config.bin_dir.join("pre-commit.sample").exists());
    assert!(!f.config.bin_dir.join("bin").exists());
}

#[test]
fn test_source_discovery_prefers_staging_over_source_tree() {
    let f = Fixture::new();
    let archive = f.temp.path().join("staged-src.tar.gz");
    make_tar_gz(
        &archive,
        &[
            (".git/hooks/applypatch-msg.sample", b"#!/bin/sh\n:\n", 0o755),
            ("bin/decoy", b"#!/bin/sh\necho decoy\n", 0o755),
            ("Makefile", b"all:\n\t@true\ninstall:\n\t@true\n", 0o644),
        ],
    );

    // Simulate a make install target that installs into $(DESTDIR)/usr/local/bin.
    let exec = FakeExecutor::new().on_run(|argv, _cwd| {
        if argv.iter().any(|a| a == "install") {
            if let Some(destdir) = argv.iter().find_map(|a| a.strip_prefix("DESTDIR=")) {
                let bin = Path::new(destdir).join("usr/local/bin");
                fs::create_dir_all(&bin).unwrap();
                let exe = bin.join("real-cmd");
                fs::write(&exe, "#!/bin/sh\necho real\n").unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
                }
            }
        }
    });
    let installer = f.installer_with(Arc::new(exec));

    let def = json!({
        "schema_version": 1,
        "id": "staged-src",
        "name": "staged",
        "version": "1.0.0",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", archive.display())},
        "build": {"system": "make"}
    });

    installer.install_source(&def, true).unwrap();

    assert_eq!(
        provided_binaries(&f, "staged-src"),
        vec!["real-cmd".to_string()]
    );
    assert!(f.config.bin_dir.join("real-cmd").exists());
    assert!(
        !f.config.bin_dir.join("decoy").exists(),
        "source-tree binaries must not be exposed when staging has commands"
    );
    assert!(!f.config.bin_dir.join("applypatch-msg.sample").exists());
}

#[test]
fn test_tree_and_cloc_style_packages_do_not_conflict_over_git_hooks() {
    let f = Fixture::new();
    // Real executor: the custom "chmod +x cloc" step must actually run.
    let installer = f.installer();

    // tree-src style: make installs into $(DESTDIR)/usr/local/bin.
    let tree_archive = f.temp.path().join("tree.tar.gz");
    make_tar_gz(
        &tree_archive,
        &[
            (".git/hooks/applypatch-msg.sample", b"#!/bin/sh\n:\n", 0o755),
            ("tree", b"#!/bin/sh\necho tree\n", 0o755),
            ("Makefile", b"all:\n\t@true\ninstall:\n\t@true\n", 0o644),
        ],
    );

    // cloc-src style: no install target; custom steps make a root script executable.
    let cloc_archive = f.temp.path().join("cloc.tar.gz");
    make_tar_gz(
        &cloc_archive,
        &[
            (".git/hooks/applypatch-msg.sample", b"#!/bin/sh\n:\n", 0o755),
            ("cloc", b"#!/usr/bin/env perl\nprint \"cloc\\n\";\n", 0o644),
        ],
    );

    let tree_def = json!({
        "schema_version": 1,
        "id": "tree-src",
        "name": "tree",
        "version": "2.3.2",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", tree_archive.display())},
        "build": {"system": "make"}
    });
    let cloc_def = json!({
        "schema_version": 1,
        "id": "cloc-src",
        "name": "cloc",
        "version": "2.10",
        "type": "source",
        "source": {"type": "tar.gz", "url": format!("file://{}", cloc_archive.display())},
        "build": {"steps": ["chmod +x cloc"]}
    });

    installer.install_source(&tree_def, true).unwrap();
    installer.install_source(&cloc_def, true).unwrap();

    assert_eq!(provided_binaries(&f, "tree-src"), vec!["tree".to_string()]);
    assert_eq!(provided_binaries(&f, "cloc-src"), vec!["cloc".to_string()]);
    assert!(f.config.bin_dir.join("tree").exists());
    assert!(f.config.bin_dir.join("cloc").exists());
    assert!(!f.config.bin_dir.join("applypatch-msg.sample").exists());
}
