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
fn test_search_output_formatting_single_vs_multi_repo() {
    let f = Fixture::new();

    f.catalogue().update_all().unwrap();

    // With 1 repo configured (default fixture setup has 1 repo "default"):
    let out1 = f.run_cli(&["search", "fastfetch"]);
    assert_eq!(code(&out1), 0);
    let stdout1 = stdout(&out1);
    assert!(stdout1.contains("PACKAGE ID           TYPE       DESCRIPTION"));
    assert!(!stdout1.contains("REPO"));
    assert!(stdout1.contains("fastfetch-bin        binary     Precompiled fastfetch"));

    // Add a second repository
    let repo_b_dir = f.temp.path().join("repo_b");
    fs::create_dir_all(repo_b_dir.join("packages")).unwrap();
    write_json(
        &repo_b_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "fastfetch-bin": {"definition": "packages/fastfetch-bin.json", "type": "binary", "description": "Fastfetch from Repo B"}
            }
        }),
    );
    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": f.cat_dir.to_string_lossy(), "priority": 100},
            "repo-b": {"name": "repo-b", "url": repo_b_dir.to_string_lossy(), "priority": 200}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();

    // With > 1 repos configured:
    let out2 = f.run_cli(&["search", "fastfetch"]);
    assert_eq!(code(&out2), 0);
    let stdout2 = stdout(&out2);
    assert!(stdout2.contains("PACKAGE ID      REPO     TYPE       DESCRIPTION"));
    assert!(stdout2.contains("fastfetch-bin   repo-b   binary     Fastfetch from Repo B"));

    // Add a third repository with a long repository name (>8 chars)
    let repo_long_dir = f.temp.path().join("repo_long");
    fs::create_dir_all(repo_long_dir.join("packages")).unwrap();
    write_json(
        &repo_long_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "long-app": {"definition": "packages/long-app.json", "type": "binary", "description": "App in long repo"}
            }
        }),
    );
    let repos_long = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": f.cat_dir.to_string_lossy(), "priority": 100},
            "my-extra-long-repository-name": {"name": "my-extra-long-repository-name", "url": repo_long_dir.to_string_lossy(), "priority": 300}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos_long)
        .unwrap();

    f.catalogue().update_all().unwrap();

    let out3 = f.run_cli(&["search", "long-app"]);
    assert_eq!(code(&out3), 0);
    let stdout3 = stdout(&out3);
    assert!(
        stdout3.contains("PACKAGE ID      REPO                            TYPE       DESCRIPTION")
    );
    assert!(stdout3
        .contains("long-app        my-extra-long-repository-name   binary     App in long repo"));
}

#[test]
fn test_internal_complete() {
    let f = Fixture::new();
    f.catalogue().update_all().unwrap();

    let out_avail = f.run_cli(&["_complete", "available"]);
    assert_eq!(code(&out_avail), 0);
    let stdout_avail = stdout(&out_avail);
    assert!(stdout_avail.contains("fastfetch-bin"));
    assert!(!stdout_avail.contains('\t'));

    let out_repos = f.run_cli(&["_complete", "repos"]);
    assert_eq!(code(&out_repos), 0);
    let stdout_repos = stdout(&out_repos);
    assert!(stdout_repos.contains("default"));
    assert!(!stdout_repos.contains('\t'));

    f.installer().install("fastfetch-bin", true, false).unwrap();

    let out_inst = f.run_cli(&["_complete", "installed"]);
    assert_eq!(code(&out_inst), 0);
    let stdout_inst = stdout(&out_inst);
    assert!(stdout_inst.contains("fastfetch-bin"));
    assert!(!stdout_inst.contains('\t'));
}

#[test]
fn test_completions_fish() {
    let f = Fixture::new();
    let out = f.run_cli(&["completions", "fish"]);
    assert_eq!(code(&out), 0);
    let stdout_str = stdout(&out);
    assert!(stdout_str.contains("complete -c aster -f"));
    assert!(stdout_str.contains("install"));
    assert!(stdout_str.contains("remove"));
    assert!(stdout_str.contains("search"));
    assert!(stdout_str.contains("-f -a \"(aster _complete available 2>/dev/null)\""));
    assert!(stdout_str.contains("-f -a \"(aster _complete installed 2>/dev/null)\""));
    assert!(stdout_str.contains("-f -a \"(aster _complete repos 2>/dev/null)\""));
    assert!(!stdout_str.contains("-a \"_complete\""));
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

    let err = installer.install_source(&fd_def, true, false).unwrap_err();
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

    installer.install_source(&fd_def, true, false).unwrap();

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
    installer.install_source(&src_pkg_def, true, false).unwrap();
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

    installer.install_source(&package, true, false).unwrap();

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

    installer.install_source(&def, true, false).unwrap();
    installer.install_source(&def, true, false).unwrap();
    installer.install_source(&def, true, false).unwrap();

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

    f.installer().install("comm-pkg", true, false).unwrap();

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

    installer.install_source(&def, true, false).unwrap();

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

    installer.install_source(&def, true, false).unwrap();

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

    installer.install_source(&tree_def, true, false).unwrap();
    installer.install_source(&cloc_def, true, false).unwrap();

    assert_eq!(provided_binaries(&f, "tree-src"), vec!["tree".to_string()]);
    assert_eq!(provided_binaries(&f, "cloc-src"), vec!["cloc".to_string()]);
    assert!(f.config.bin_dir.join("tree").exists());
    assert!(f.config.bin_dir.join("cloc").exists());
    assert!(!f.config.bin_dir.join("applypatch-msg.sample").exists());
}

#[test]
fn test_history_recording() {
    let f = Fixture::new();

    let out = f.run_cli(&["history"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("No history recorded yet."));

    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);
    assert_eq!(code(&f.run_cli(&["install", "fastfetch-bin"])), 0);

    let out_hist = f.run_cli(&["history"]);
    assert_eq!(code(&out_hist), 0);
    let hist_text = stdout(&out_hist);
    assert!(hist_text.contains("Installed 'fastfetch-bin' version 2.30.0"));

    assert_eq!(code(&f.run_cli(&["remove", "fastfetch-bin"])), 0);

    let out_hist2 = f.run_cli(&["history"]);
    assert_eq!(code(&out_hist2), 0);
    let hist_text2 = stdout(&out_hist2);
    assert!(hist_text2.contains("Installed 'fastfetch-bin' version 2.30.0"));
    assert!(hist_text2.contains("Removed 'fastfetch-bin'"));
}

#[test]
fn test_install_verbose_flags() {
    let f = Fixture::new();

    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);

    let out_v = f.run_cli(&["install", "htop-src", "-y", "-v"]);
    assert_eq!(code(&out_v), 0);
    let stdout_v = stdout(&out_v);
    assert!(stdout_v.contains("$ mkdir -p staging_bin"));

    assert_eq!(code(&f.run_cli(&["remove", "htop-src"])), 0);

    let out_verbose = f.run_cli(&["install", "htop-src", "-y", "-verbose"]);
    assert_eq!(code(&out_verbose), 0);
    let stdout_verb = stdout(&out_verbose);
    assert!(stdout_verb.contains("$ mkdir -p staging_bin"));
}

#[test]
fn test_history_limit_and_local_timestamp() {
    let f = Fixture::new();

    f.config.log_history("Entry 1");
    f.config.log_history("Entry 2");
    f.config.log_history("Entry 3");

    let out_all = f.run_cli(&["history"]);
    assert_eq!(code(&out_all), 0);
    let text_all = stdout(&out_all);
    assert!(text_all.contains("Entry 1"));
    assert!(text_all.contains("Entry 2"));
    assert!(text_all.contains("Entry 3"));

    let out_limit = f.run_cli(&["history", "--limit", "2"]);
    assert_eq!(code(&out_limit), 0);
    let text_limit = stdout(&out_limit);
    assert!(!text_limit.contains("Entry 1"));
    assert!(text_limit.contains("Entry 2"));
    assert!(text_limit.contains("Entry 3"));

    let re = regex::Regex::new(r"\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\] Entry 3").unwrap();
    assert!(re.is_match(&text_limit));
}

#[test]
fn test_install_dry_run() {
    let f = Fixture::new();
    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);

    let out = f.run_cli(&["install", "--dry-run", "app-with-dep"]);
    assert_eq!(code(&out), 0);
    let stdout_text = stdout(&out);
    assert!(stdout_text.contains("Dry run: Installation plan for 'app-with-dep':"));
    assert!(stdout_text.contains("dep-lib"));
    assert!(stdout_text.contains("app-with-dep"));

    // Ensure package was NOT installed
    assert_eq!(code(&f.run_cli(&["installed", "app-with-dep"])), 1);
}

#[test]
fn test_verify_intact_and_broken() {
    let f = Fixture::new();
    assert_eq!(code(&f.run_cli(&["repo", "update"])), 0);

    assert_eq!(code(&f.run_cli(&["verify"])), 0);

    assert_eq!(code(&f.run_cli(&["install", "fastfetch-bin"])), 0);

    // Verify all intact
    let out_verify = f.run_cli(&["verify"]);
    assert_eq!(code(&out_verify), 0);
    assert!(stdout(&out_verify).contains("All installed packages are intact."));

    let out_single = f.run_cli(&["verify", "fastfetch-bin"]);
    assert_eq!(code(&out_single), 0);
    assert!(stdout(&out_single).contains("Package 'fastfetch-bin' is intact."));

    // Add a broken symlink inside package directory recorded as installed_file
    let pkg_dir = f.config.packages_dir.join("fastfetch-bin");
    let broken_link = pkg_dir.join("broken_file_link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(pkg_dir.join("nonexistent_target"), &broken_link).unwrap();

    let mut reg = f.registry().load_registry().unwrap();
    if let Some(serde_json::Value::Object(map)) = reg.get_mut("installed") {
        if let Some(serde_json::Value::Object(pkg_map)) = map.get_mut("fastfetch-bin") {
            if let Some(serde_json::Value::Array(arr)) = pkg_map.get_mut("installed_files") {
                arr.push(serde_json::json!("broken_file_link"));
            }
        }
    }
    f.registry().save_registry(&reg).unwrap();

    #[cfg(unix)]
    {
        let out_broken_sym = f.run_cli(&["verify", "fastfetch-bin"]);
        assert_ne!(code(&out_broken_sym), 0);
        assert!(stdout(&out_broken_sym).contains("Broken recorded symlink: broken_file_link"));
    }

    // Break symlink
    let link_path = f.config.bin_dir.join("fastfetch");
    fs::remove_file(&link_path).unwrap();

    let out_broken = f.run_cli(&["verify", "fastfetch-bin"]);
    assert_ne!(code(&out_broken), 0);
    assert!(stdout(&out_broken).contains("Missing command symlink"));

    let out_all_failed = f.run_cli(&["verify"]);
    assert_ne!(code(&out_all_failed), 0);
}

#[test]
fn test_safe_upgrade_and_rollback() {
    let f = Fixture::new();

    // Create repo with v1 of pkg
    let repo_dir = f.temp.path().join("upg_repo");
    fs::create_dir_all(repo_dir.join("packages")).unwrap();

    let v1_archive = f.temp.path().join("pkg-1.0.0.tar.gz");
    make_tar_gz(&v1_archive, &[("bin/mypkg", b"#!/bin/sh\necho v1", 0o755)]);

    write_json(
        &repo_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "mypkg": {"definition": "packages/mypkg.json", "type": "binary", "description": "My Pkg"}
            }
        }),
    );
    write_json(
        &repo_dir.join("packages/mypkg.json"),
        &json!({
            "schema_version": 1,
            "id": "mypkg",
            "name": "mypkg",
            "version": "1.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": format!("file://{}", v1_archive.display())},
                "linux-aarch64": {"url": format!("file://{}", v1_archive.display())}
            }
        }),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": repo_dir.to_string_lossy(), "priority": 100}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();
    f.installer().install("mypkg", true, false).unwrap();

    let bin_link = f.config.bin_dir.join("mypkg");
    assert!(bin_link.exists());

    // Update repo to v2 with a NON-EXISTENT archive so installation fails
    write_json(
        &repo_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "mypkg": {"definition": "packages/mypkg.json", "type": "binary", "description": "My Pkg v2"}
            }
        }),
    );
    write_json(
        &repo_dir.join("packages/mypkg.json"),
        &json!({
            "schema_version": 1,
            "id": "mypkg",
            "name": "mypkg",
            "version": "2.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": "file:///nonexistent/path/pkg-2.0.0.tar.gz"},
                "linux-aarch64": {"url": "file:///nonexistent/path/pkg-2.0.0.tar.gz"}
            }
        }),
    );
    f.catalogue().update_all().unwrap();

    let err = f
        .installer()
        .upgrade_package("mypkg", true, false)
        .unwrap_err();
    assert!(err.to_string().contains("Upgrade failed"));

    // Verify rollback restored v1.0.0
    assert!(f.registry().is_installed("mypkg").unwrap());
    let info = f
        .registry()
        .get_installed_package("mypkg")
        .unwrap()
        .unwrap();
    assert_eq!(info["version"], "1.0.0");
    assert!(bin_link.exists());
    assert!(f.config.packages_dir.join("mypkg").exists());
    assert!(!f.config.packages_dir.join("mypkg.upgrade_backup").exists());
}

#[test]
fn test_upgrade_cancelled_build_steps_rollback() {
    let f = Fixture::new();

    let repo_dir = f.temp.path().join("cancel_repo");
    fs::create_dir_all(repo_dir.join("packages")).unwrap();

    let v1_archive = f.temp.path().join("steps-1.0.0.tar.gz");
    make_tar_gz(&v1_archive, &[("myscript", b"#!/bin/sh\necho v1", 0o755)]);

    write_json(
        &repo_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "stepspkg": {"definition": "packages/stepspkg.json", "type": "source", "description": "Steps Pkg"}
            }
        }),
    );
    write_json(
        &repo_dir.join("packages/stepspkg.json"),
        &json!({
            "schema_version": 1,
            "id": "stepspkg",
            "name": "stepspkg",
            "version": "1.0.0",
            "type": "source",
            "source": {"type": "tar.gz", "url": format!("file://{}", v1_archive.display())},
            "build": {"steps": ["chmod +x myscript"]},
            "executables": ["myscript"]
        }),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": repo_dir.to_string_lossy(), "priority": 100}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();
    f.installer().install("stepspkg", true, false).unwrap();

    assert!(f.config.bin_dir.join("myscript").exists());

    // Update repo to v2 with custom steps. Without auto_yes (auto_yes=false) and non-interactive stdin ("n"),
    // execution prompt will be declined.
    write_json(
        &repo_dir.join("packages/stepspkg.json"),
        &json!({
            "schema_version": 1,
            "id": "stepspkg",
            "name": "stepspkg",
            "version": "2.0.0",
            "type": "source",
            "source": {"type": "tar.gz", "url": format!("file://{}", v1_archive.display())},
            "build": {"steps": ["echo v2"]},
            "executables": ["myscript"]
        }),
    );
    f.catalogue().update_all().unwrap();

    let err = f
        .installer()
        .upgrade_package("stepspkg", false, false)
        .unwrap_err();
    assert!(err.to_string().contains("cancelled"));

    // Verify rollback restored v1.0.0 and package remains installed and usable
    assert!(f.registry().is_installed("stepspkg").unwrap());
    let info = f
        .registry()
        .get_installed_package("stepspkg")
        .unwrap()
        .unwrap();
    assert_eq!(info["version"], "1.0.0");
    assert!(f.config.bin_dir.join("myscript").exists());
}

#[test]
fn test_upgrade_binary_names_change_removes_stale_symlinks() {
    let f = Fixture::new();

    let repo_dir = f.temp.path().join("bin_change_repo");
    fs::create_dir_all(repo_dir.join("packages")).unwrap();

    let v1_archive = f.temp.path().join("rename-1.0.0.tar.gz");
    make_tar_gz(&v1_archive, &[("bin/oldcmd", b"#!/bin/sh\necho v1", 0o755)]);

    write_json(
        &repo_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "renamepkg": {"definition": "packages/renamepkg.json", "type": "binary", "description": "Rename Pkg"}
            }
        }),
    );
    write_json(
        &repo_dir.join("packages/renamepkg.json"),
        &json!({
            "schema_version": 1,
            "id": "renamepkg",
            "name": "renamepkg",
            "version": "1.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": format!("file://{}", v1_archive.display())},
                "linux-aarch64": {"url": format!("file://{}", v1_archive.display())}
            }
        }),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": repo_dir.to_string_lossy(), "priority": 100}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();
    f.installer().install("renamepkg", true, false).unwrap();

    let old_link = f.config.bin_dir.join("oldcmd");
    assert!(old_link.exists());

    // Update repo to v2 providing newcmd instead of oldcmd
    let v2_archive = f.temp.path().join("rename-2.0.0.tar.gz");
    make_tar_gz(&v2_archive, &[("bin/newcmd", b"#!/bin/sh\necho v2", 0o755)]);

    write_json(
        &repo_dir.join("packages/renamepkg.json"),
        &json!({
            "schema_version": 1,
            "id": "renamepkg",
            "name": "renamepkg",
            "version": "2.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": format!("file://{}", v2_archive.display())},
                "linux-aarch64": {"url": format!("file://{}", v2_archive.display())}
            }
        }),
    );
    f.catalogue().update_all().unwrap();

    f.installer()
        .upgrade_package("renamepkg", true, false)
        .unwrap();

    let new_link = f.config.bin_dir.join("newcmd");
    assert!(new_link.exists());
    assert!(
        !aster::util::exists_or_symlink(&old_link) && !old_link.is_symlink(),
        "stale command link should have been removed"
    );
}

#[test]
fn test_upgrade_retry_with_existing_backup_errors() {
    let f = Fixture::new();

    let repo_dir = f.temp.path().join("retry_repo");
    fs::create_dir_all(repo_dir.join("packages")).unwrap();

    let v1_archive = f.temp.path().join("retry-1.0.0.tar.gz");
    make_tar_gz(
        &v1_archive,
        &[("bin/retrypkg", b"#!/bin/sh\necho v1", 0o755)],
    );

    write_json(
        &repo_dir.join("index.json"),
        &json!({
            "schema_version": 1,
            "packages": {
                "retrypkg": {"definition": "packages/retrypkg.json", "type": "binary", "description": "Retry Pkg"}
            }
        }),
    );
    write_json(
        &repo_dir.join("packages/retrypkg.json"),
        &json!({
            "schema_version": 1,
            "id": "retrypkg",
            "name": "retrypkg",
            "version": "1.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": format!("file://{}", v1_archive.display())},
                "linux-aarch64": {"url": format!("file://{}", v1_archive.display())}
            }
        }),
    );

    let repos = json!({
        "schema_version": 1,
        "repositories": {
            "default": {"name": "default", "url": repo_dir.to_string_lossy(), "priority": 100}
        }
    });
    f.config
        .save_json_atomic(&f.config.repositories_json, &repos)
        .unwrap();

    f.catalogue().update_all().unwrap();
    f.installer().install("retrypkg", true, false).unwrap();

    // Manually create a recovery backup directory to simulate a previously preserved backup
    let backup_dir = f.config.packages_dir.join("retrypkg.upgrade_backup");
    fs::create_dir_all(&backup_dir).unwrap();
    fs::write(backup_dir.join("preserved_file"), b"important").unwrap();

    // Attempt upgrade, verify it refuses to overwrite the recovery backup
    let err = f
        .installer()
        .upgrade_package("retrypkg", true, false)
        .unwrap_err();
    assert!(err.to_string().contains("recovery backup"));

    // Verify backup was NOT deleted
    assert!(backup_dir.join("preserved_file").exists());
}
