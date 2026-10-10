import json
import pytest
import shutil
import tempfile
import stat
import zipfile
import tarfile
from pathlib import Path
from aster.config import AsterConfig
from aster.registry import RegistryManager
from aster.catalogue import CatalogueManager
from aster.installer import PackageInstaller
from aster.version import compare_versions
from aster.resolver import DependencyResolver, DependencyError
from aster.cli import main
from aster.schema import ValidationError, validate_package_definition, validate_index, validate_registry

@pytest.fixture
def temp_aster_env(monkeypatch):
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_path = Path(tmpdir)
        monkeypatch.setenv("ASTER_HOME", str(tmp_path / "aster"))

        cat_dir = tmp_path / "catalogue"
        cat_dir.mkdir()
        pkgs_dir = cat_dir / "packages"
        pkgs_dir.mkdir()

        index_data = {
            "schema_version": 1,
            "packages": {
                "fastfetch-bin": {
                    "definition": "packages/fastfetch-bin.json",
                    "type": "binary",
                    "description": "Precompiled fastfetch"
                },
                "test-src": {
                    "definition": "packages/test-src.json",
                    "type": "source",
                    "description": "Test source package"
                },
                "dep-lib": {
                    "definition": "packages/dep-lib.json",
                    "type": "binary",
                    "description": "Dependency library"
                },
                "app-with-dep": {
                    "definition": "packages/app-with-dep.json",
                    "type": "binary",
                    "description": "App requiring dep-lib"
                }
            }
        }
        with open(cat_dir / "index.json", "w") as f:
            json.dump(index_data, f)

        # Binary archive asset
        bin_asset_dir = tmp_path / "assets"
        bin_asset_dir.mkdir()
        bin_exe = bin_asset_dir / "fastfetch"
        bin_exe.write_text("#!/bin/sh\necho fastfetch")
        bin_exe.chmod(0o755)

        bin_archive = bin_asset_dir / "fastfetch-bin.tar.gz"
        with tarfile.open(bin_archive, "w:gz") as tar:
            tar.add(bin_exe, arcname="bin/fastfetch")

        fastfetch_bin_def = {
            "schema_version": 1,
            "id": "fastfetch-bin",
            "name": "Fastfetch",
            "version": "2.30.0",
            "type": "binary",
            "description": "Precompiled fastfetch",
            "downloads": {
                "linux-x86_64": {
                    "url": f"file://{bin_archive}",
                    "sha256": "EXPECTED_SHA256"
                },
                "linux-aarch64": {
                    "url": f"file://{bin_archive}",
                    "sha256": "EXPECTED_SHA256"
                }
            },
            "supported_platforms": ["linux-x86_64", "linux-aarch64"]
        }
        with open(pkgs_dir / "fastfetch-bin.json", "w") as f:
            json.dump(fastfetch_bin_def, f)

        # Source asset
        src_asset_dir = tmp_path / "src_asset"
        src_asset_dir.mkdir()
        (src_asset_dir / "Makefile").write_text("all:\n\t@echo 'build complete'\ninstall:\n\tmkdir -p $(DESTDIR)/bin\n\techo '#!/bin/sh' > $(DESTDIR)/bin/test-src\n\tchmod +x $(DESTDIR)/bin/test-src\n")

        src_archive = tmp_path / "test-src.tar.gz"
        with tarfile.open(src_archive, "w:gz") as tar:
            tar.add(src_asset_dir / "Makefile", arcname="Makefile")

        test_src_def = {
            "schema_version": 1,
            "id": "test-src",
            "name": "Test Source",
            "version": "1.0.0",
            "type": "source",
            "description": "Test source package",
            "source": {
                "type": "tar.gz",
                "url": f"file://{src_archive}"
            },
            "build": {
                "system": "make"
            }
        }
        with open(pkgs_dir / "test-src.json", "w") as f:
            json.dump(test_src_def, f)

        # Nested binary archive fixture e.g. ripgrep-bin
        ripgrep_bin_dir = tmp_path / "ripgrep_asset"
        ripgrep_bin_dir.mkdir()
        ripgrep_exe = ripgrep_bin_dir / "rg"
        ripgrep_exe.write_text("#!/bin/sh\necho rg")
        ripgrep_exe.chmod(0o755)

        ripgrep_archive = tmp_path / "ripgrep-14.1.0-x86_64-linux.tar.gz"
        with tarfile.open(ripgrep_archive, "w:gz") as tar:
            # Add file under top-level folder e.g. ripgrep-14.1.0-x86_64/rg
            tar.add(ripgrep_exe, arcname="ripgrep-14.1.0-x86_64/rg")

        ripgrep_bin_def = {
            "schema_version": 1,
            "id": "ripgrep-bin",
            "name": "ripgrep",
            "version": "14.1.0",
            "type": "binary",
            "description": "line-oriented search tool",
            "downloads": {
                "linux-x86_64": {"url": f"file://{ripgrep_archive}"},
                "linux-aarch64": {"url": f"file://{ripgrep_archive}"}
            }
        }
        with open(pkgs_dir / "ripgrep-bin.json", "w") as f:
            json.dump(ripgrep_bin_def, f)

        index_data["packages"]["ripgrep-bin"] = {
            "definition": "packages/ripgrep-bin.json",
            "type": "binary",
            "description": "line-oriented search tool"
        }

        # Custom steps package fixture e.g. htop-src
        htop_src_def = {
            "schema_version": 1,
            "id": "htop-src",
            "name": "htop",
            "version": "3.5.3",
            "type": "source",
            "description": "Interactive process viewer",
            "source": {
                "type": "tar.gz",
                "url": f"file://{src_archive}"
            },
            "build": {
                "steps": [
                    "mkdir -p staging_bin",
                    "echo '#!/bin/sh' > staging_bin/htop",
                    "chmod +x staging_bin/htop"
                ]
            }
        }
        with open(pkgs_dir / "htop-src.json", "w") as f:
            json.dump(htop_src_def, f)

        index_data["packages"]["htop-src"] = {
            "definition": "packages/htop-src.json",
            "type": "source",
            "description": "Interactive process viewer"
        }
        with open(cat_dir / "index.json", "w") as f:
            json.dump(index_data, f)

        # Dependency packages
        dep_lib_exe = bin_asset_dir / "dep-lib"
        dep_lib_exe.write_text("#!/bin/sh\necho dep-lib")
        dep_lib_exe.chmod(0o755)
        dep_lib_archive = bin_asset_dir / "dep-lib.tar.gz"
        with tarfile.open(dep_lib_archive, "w:gz") as tar:
            tar.add(dep_lib_exe, arcname="bin/dep-lib")

        dep_lib_def = {
            "schema_version": 1,
            "id": "dep-lib",
            "name": "DepLib",
            "version": "1.0.0",
            "type": "binary",
            "downloads": {
                "linux-x86_64": {"url": f"file://{dep_lib_archive}"},
                "linux-aarch64": {"url": f"file://{dep_lib_archive}"}
            }
        }
        with open(pkgs_dir / "dep-lib.json", "w") as f:
            json.dump(dep_lib_def, f)

        app_dep_exe = bin_asset_dir / "app-dep"
        app_dep_exe.write_text("#!/bin/sh\necho app")
        app_dep_exe.chmod(0o755)
        app_dep_archive = bin_asset_dir / "app-dep.tar.gz"
        with tarfile.open(app_dep_archive, "w:gz") as tar:
            tar.add(app_dep_exe, arcname="bin/app-dep")

        app_dep_def = {
            "schema_version": 1,
            "id": "app-with-dep",
            "name": "AppWithDep",
            "version": "1.0.0",
            "type": "binary",
            "dependencies": ["dep-lib"],
            "downloads": {
                "linux-x86_64": {"url": f"file://{app_dep_archive}"},
                "linux-aarch64": {"url": f"file://{app_dep_archive}"}
            }
        }
        with open(pkgs_dir / "app-with-dep.json", "w") as f:
            json.dump(app_dep_def, f)

        config = AsterConfig()
        config.ensure_directories()

        repos_data = {
            "schema_version": 1,
            "repositories": {
                "default": {
                    "name": "default",
                    "url": str(cat_dir)
                }
            }
        }
        config.save_json_atomic(config.repositories_json, repos_data)

        yield config

def test_schema_validation():
    with pytest.raises(ValidationError):
        validate_package_definition({})
    with pytest.raises(ValidationError):
        validate_package_definition({"schema_version": 1, "id": "x", "name": "x", "version": "1", "type": "invalid"})

    validate_package_definition({"schema_version": 1, "id": "x", "name": "x", "version": "1", "type": "binary"})

def test_version_comparison():
    assert compare_versions("1.0.0", "1.0.1") == -1
    assert compare_versions("2.0.0", "1.9.9") == 1
    assert compare_versions("1.0.0", "1.0.0") == 0
    assert compare_versions("1.0.0-rc1", "1.0.0") == -1

def test_config_and_registry(temp_aster_env):
    config = temp_aster_env
    registry = RegistryManager(config)
    assert not registry.is_installed("fastfetch-bin")
    assert registry.list_installed() == {}

def test_catalogue_search_and_update(temp_aster_env):
    config = temp_aster_env
    catalogue = CatalogueManager(config)
    catalogue.update_all()

    results = catalogue.search_packages("fastfetch")
    assert "fastfetch-bin" in results

    pkg_def = catalogue.get_package_definition("fastfetch-bin")
    assert pkg_def["id"] == "fastfetch-bin"

def test_dependency_resolution(temp_aster_env):
    config = temp_aster_env
    catalogue = CatalogueManager(config)
    registry = RegistryManager(config)
    catalogue.update_all()

    resolver = DependencyResolver(catalogue, registry)
    plan = resolver.resolve_dependencies("app-with-dep")
    assert plan == ["dep-lib", "app-with-dep"]

def test_cli_full_workflow(temp_aster_env, capsys):
    config = temp_aster_env

    # 1. Update repo
    assert main(["repo", "update"]) == 0

    # 2. Search
    assert main(["search", "fastfetch"]) == 0
    captured = capsys.readouterr()
    assert "fastfetch-bin" in captured.out

    # 3. Check installed status (not installed)
    assert main(["installed", "fastfetch-bin"]) == 1

    # 4. Install binary package
    assert main(["install", "fastfetch-bin"]) == 0

    # 5. Check installed status
    assert main(["installed", "fastfetch-bin"]) == 0

    # 6. Check list
    assert main(["list"]) == 0
    captured = capsys.readouterr()
    assert "fastfetch-bin" in captured.out

    # 7. Check info
    assert main(["info", "fastfetch-bin"]) == 0

    # 8. Check binary link created
    bin_link = config.bin_dir / "fastfetch"
    assert bin_link.exists()

    # 9. Test doctor command
    assert main(["doctor"]) == 0

    # 10. Test clean command
    assert main(["clean"]) == 0

    # 11. Remove package
    assert main(["remove", "fastfetch-bin"]) == 0
    assert not bin_link.exists()
    assert main(["installed", "fastfetch-bin"]) == 1

def test_install_with_dependencies(temp_aster_env):
    config = temp_aster_env
    assert main(["repo", "update"]) == 0
    assert main(["install", "app-with-dep"]) == 0

    assert (config.bin_dir / "dep-lib").exists()
    assert (config.bin_dir / "app-dep").exists()

    # Attempt removal of dependency should fail
    assert main(["remove", "dep-lib"]) == 1

    # Force removal succeeds
    assert main(["remove", "dep-lib", "--force"]) == 0

def test_install_custom_steps_package(temp_aster_env):
    config = temp_aster_env
    assert main(["repo", "update"]) == 0
    # Test installation with -y auto confirmation
    assert main(["install", "htop-src", "-y"]) == 0
    assert (config.bin_dir / "htop").exists()
    assert main(["remove", "htop-src"]) == 0

def test_install_nested_binary_package(temp_aster_env):
    config = temp_aster_env
    assert main(["repo", "update"]) == 0
    assert main(["install", "ripgrep-bin"]) == 0
    assert (config.bin_dir / "rg").exists()
    assert main(["remove", "ripgrep-bin"]) == 0

def test_missing_declared_executable_validation(temp_aster_env, monkeypatch, tmp_path):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    dummy_tar = tmp_path / "dummy.tar.gz"
    with tarfile.open(dummy_tar, "w:gz") as tar:
        pass

    fd_def = {
        "schema_version": 1,
        "id": "fd-src",
        "name": "fd",
        "version": "10.0.0",
        "type": "source",
        "source": {
            "type": "tar.gz",
            "url": f"file://{dummy_tar}"
        },
        "executables": ["fd"],
        "build": {
            "system": "cargo"
        }
    }

    # Mock subprocess.run for cargo build success
    def fake_run(cmd, *args, **kwargs):
        class DummyRes:
            returncode = 0
            stdout = ""
            stderr = ""
        return DummyRes()

    monkeypatch.setattr("subprocess.run", fake_run)
    monkeypatch.setattr("shutil.which", lambda cmd, path=None: "/usr/bin/cargo")

    with pytest.raises(RuntimeError, match="Declared executable 'fd' was not produced"):
        installer._install_source(fd_def, auto_yes=True)

def test_cargo_detection_system_vs_isolated(temp_aster_env, monkeypatch, tmp_path):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    dummy_tar = tmp_path / "dummy.tar.gz"
    with tarfile.open(dummy_tar, "w:gz") as tar:
        pass

    recorded_envs = []

    def fake_run(cmd, *args, **kwargs):
        env = kwargs.get("env", {})
        recorded_envs.append(env)
        # Create a fake binary in target/release/fd so executable validation succeeds
        cwd = kwargs.get("cwd")
        if cwd and "cargo" in cmd[0]:
            target_dir = Path(cwd) / "target" / "release"
            target_dir.mkdir(parents=True, exist_ok=True)
            fd_bin = target_dir / "fd"
            fd_bin.write_text("#!/bin/sh\necho fd")
            fd_bin.chmod(0o755)
        class DummyRes:
            returncode = 0
            stdout = ""
            stderr = ""
        return DummyRes()

    monkeypatch.setattr("subprocess.run", fake_run)
    monkeypatch.setattr("shutil.which", lambda cmd, path=None: "/usr/bin/cargo" if cmd == "cargo" else None)

    fd_def = {
        "schema_version": 1,
        "id": "fd-src",
        "name": "fd",
        "version": "10.0.0",
        "type": "source",
        "source": {
            "type": "tar.gz",
            "url": f"file://{dummy_tar}"
        },
        "executables": ["fd"],
        "build": {
            "system": "cargo"
        }
    }

    installer._install_source(fd_def, auto_yes=True)

    # When system cargo is used, RUSTUP_HOME and CARGO_HOME should NOT be overridden with Aster's toolchain dir
    build_env_used = recorded_envs[0]
    assert "RUSTUP_HOME" not in build_env_used or build_env_used["RUSTUP_HOME"] != str(config.rust_toolchain_dir)
    assert "CARGO_HOME" not in build_env_used or build_env_used["CARGO_HOME"] != str(config.rust_toolchain_dir)

def test_install_flattening_name_collision(temp_aster_env, tmp_path):
    # Tests installing an archive where the single top-level directory has the same name
    # as an executable inside it (e.g. age-bin with staging-age-bin/age/age)
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    age_exe = tmp_path / "age_exe"
    age_exe.write_text("#!/bin/sh\necho age")
    age_exe.chmod(0o755)

    age_archive = tmp_path / "age-v1.1.1-linux-amd64.tar.gz"
    with tarfile.open(age_archive, "w:gz") as tar:
        # Top level directory "age", containing executable file "age"
        tar.add(age_exe, arcname="age/age")

    age_bin_def = {
        "schema_version": 1,
        "id": "age-bin",
        "name": "age",
        "version": "1.1.1",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": f"file://{age_archive}"},
            "linux-aarch64": {"url": f"file://{age_archive}"}
        }
    }

    installer._install_binary(age_bin_def)
    assert registry.is_installed("age-bin")
    assert (config.bin_dir / "age").exists()

def test_install_tar_xz_binary_and_source(temp_aster_env, tmp_path):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    # 1. Test binary package with .tar.xz
    bin_exe = tmp_path / "xzbin"
    bin_exe.write_text("#!/bin/sh\necho xzbin")
    bin_exe.chmod(0o755)

    bin_tar_xz = tmp_path / "xzbin-1.0.0.tar.xz"
    with tarfile.open(bin_tar_xz, "w:xz") as tar:
        tar.add(bin_exe, arcname="bin/xzbin")

    bin_pkg_def = {
        "schema_version": 1,
        "id": "xzbin-bin",
        "name": "XZBin",
        "version": "1.0.0",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": f"file://{bin_tar_xz}"},
            "linux-aarch64": {"url": f"file://{bin_tar_xz}"}
        }
    }

    installer._install_binary(bin_pkg_def)
    assert registry.is_installed("xzbin-bin")
    assert (config.bin_dir / "xzbin").exists()

    # 2. Test source package with .tar.xz
    src_dir = tmp_path / "xz_src"
    src_dir.mkdir()
    (src_dir / "Makefile").write_text("all:\n\t@echo 'xz src complete'\ninstall:\n\tmkdir -p $(DESTDIR)/bin\n\techo '#!/bin/sh' > $(DESTDIR)/bin/xzsrc\n\tchmod +x $(DESTDIR)/bin/xzsrc\n")

    src_tar_xz = tmp_path / "xzsrc-1.0.0.tar.xz"
    with tarfile.open(src_tar_xz, "w:xz") as tar:
        tar.add(src_dir / "Makefile", arcname="Makefile")

    src_pkg_def = {
        "schema_version": 1,
        "id": "xzsrc-src",
        "name": "XZSrc",
        "version": "1.0.0",
        "type": "source",
        "source": {
            "type": "tar.xz",
            "url": f"file://{src_tar_xz}"
        },
        "build": {
            "system": "make"
        }
    }

    installer._install_source(src_pkg_def, auto_yes=True)
    assert registry.is_installed("xzsrc-src")
    assert (config.bin_dir / "xzsrc").exists()

def test_install_zip_preserves_executable_bits_and_discovers_multiple_binaries(temp_aster_env, tmp_path):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    archive_path = tmp_path / "yazi-release.zip"
    with zipfile.ZipFile(archive_path, "w") as archive:
        # Include a single enclosing directory to exercise Aster's archive un-nesting.
        for executable_name in ("yazi", "ya"):
            item = zipfile.ZipInfo(f"yazi-release/{executable_name}")
            item.create_system = 3  # Unix metadata, including executable permission bits.
            item.external_attr = (stat.S_IFREG | 0o755) << 16
            archive.writestr(item, f"#!/bin/sh\\necho {executable_name}\\n")

    package = {
        "schema_version": 1,
        "id": "yazi-bin",
        "name": "yazi",
        "version": "26.9.1",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {"url": f"file://{archive_path}"},
            "linux-aarch64": {"url": f"file://{archive_path}"}
        }
    }

    installer._install_binary(package)

    assert (config.bin_dir / "yazi").is_symlink()
    assert (config.bin_dir / "ya").is_symlink()
    installed = registry.get_installed_package("yazi-bin")
    assert set(installed["provided_binaries"]) == {"yazi", "ya"}


def test_cargo_custom_args_builds_declared_workspace_executables(temp_aster_env, monkeypatch, tmp_path):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    # A tiny source archive is enough because Cargo is mocked for this integration test.
    source_dir = tmp_path / "yazi-source"
    source_dir.mkdir()
    (source_dir / "Cargo.toml").write_text("[workspace]\\nmembers = []\\n")
    source_archive = tmp_path / "yazi-source.tar.gz"
    with tarfile.open(source_archive, "w:gz") as archive:
        archive.add(source_dir / "Cargo.toml", arcname="Cargo.toml")

    commands = []

    def fake_run(cmd, *args, **kwargs):
        commands.append(cmd)
        cwd = Path(kwargs["cwd"])
        target_dir = cwd / "target" / "release"
        target_dir.mkdir(parents=True, exist_ok=True)
        for executable_name in ("yazi", "ya"):
            executable = target_dir / executable_name
            executable.write_text(f"#!/bin/sh\\necho {executable_name}\\n")
            executable.chmod(0o755)

        class DummyResult:
            returncode = 0
            stdout = ""
            stderr = ""
        return DummyResult()

    monkeypatch.setattr("subprocess.run", fake_run)
    monkeypatch.setattr("shutil.which", lambda cmd, path=None: "/usr/bin/cargo" if cmd == "cargo" else None)

    package = {
        "schema_version": 1,
        "id": "yazi-src",
        "name": "yazi",
        "version": "26.9.1",
        "type": "source",
        "source": {"type": "tar.gz", "url": f"file://{source_archive}"},
        "build": {"system": "cargo", "cargo_args": ["xtask", "build"]},
        "executables": ["yazi", "ya"]
    }

    installer._install_source(package, auto_yes=True)

    assert commands == [["/usr/bin/cargo", "xtask", "build"]]
    assert (config.bin_dir / "yazi").is_symlink()
    assert (config.bin_dir / "ya").is_symlink()


def test_cleanup_on_failure(temp_aster_env):
    config = temp_aster_env
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    failing_def = {
        "schema_version": 1,
        "id": "failing-pkg",
        "name": "Failing Package",
        "version": "1.0.0",
        "type": "binary",
        "downloads": {
            "linux-x86_64": {
                "url": "file:///nonexistent/archive.tar.gz"
            }
        }
    }

    with pytest.raises(FileNotFoundError):
        installer._install_binary(failing_def)

    staging_dir = config.build_dir / "staging-failing-pkg"
    build_dir = config.build_dir / "build-failing-pkg"
    final_package_dir = config.packages_dir / "failing-pkg"

    assert not staging_dir.exists()
    assert not build_dir.exists()
    assert not final_package_dir.exists()


def test_repository_priority_resolution(temp_aster_env, tmp_path):
    config = temp_aster_env

    # Create Repo A (low priority: 50)
    repo_a_dir = tmp_path / "repo_a"
    repo_a_dir.mkdir()
    (repo_a_dir / "packages").mkdir()
    index_a = {
        "schema_version": 1,
        "packages": {
            "common-app": {
                "definition": "packages/common-app.json",
                "type": "binary",
                "description": "Common App from Repo A (low priority)"
            }
        }
    }
    with open(repo_a_dir / "index.json", "w") as f:
        json.dump(index_a, f)
    def_a = {
        "schema_version": 1,
        "id": "common-app",
        "name": "CommonAppA",
        "version": "1.0.0",
        "type": "binary"
    }
    with open(repo_a_dir / "packages" / "common-app.json", "w") as f:
        json.dump(def_a, f)

    # Create Repo B (high priority: 200)
    repo_b_dir = tmp_path / "repo_b"
    repo_b_dir.mkdir()
    (repo_b_dir / "packages").mkdir()
    index_b = {
        "schema_version": 1,
        "packages": {
            "common-app": {
                "definition": "packages/common-app.json",
                "type": "binary",
                "description": "Common App from Repo B (high priority)"
            }
        }
    }
    with open(repo_b_dir / "index.json", "w") as f:
        json.dump(index_b, f)
    def_b = {
        "schema_version": 1,
        "id": "common-app",
        "name": "CommonAppB",
        "version": "2.0.0",
        "type": "binary"
    }
    with open(repo_b_dir / "packages" / "common-app.json", "w") as f:
        json.dump(def_b, f)

    # Configure both repositories
    repos_data = {
        "schema_version": 1,
        "repositories": {
            "repo-a": {"name": "repo-a", "url": str(repo_a_dir), "priority": 50},
            "repo-b": {"name": "repo-b", "url": str(repo_b_dir), "priority": 200}
        }
    }
    config.save_json_atomic(config.repositories_json, repos_data)

    catalogue = CatalogueManager(config)
    catalogue.update_all()

    # Priority sorting check
    sorted_repos = catalogue.get_sorted_repositories()
    assert [name for name, _ in sorted_repos] == ["repo-b", "repo-a"]

    # Search check - higher priority repo info should win
    search_res = catalogue.search_packages("common-app")
    assert search_res["common-app"]["description"] == "Common App from Repo B (high priority)"
    assert search_res["common-app"]["repository"] == "repo-b"

    # Definition check - higher priority repo definition should win
    pkg_def = catalogue.get_package_definition("common-app")
    assert pkg_def["name"] == "CommonAppB"
    assert pkg_def["version"] == "2.0.0"


def test_cli_repo_priority_commands(temp_aster_env, capsys):
    config = temp_aster_env

    # 1. Add repo with priority flag
    assert main(["repo", "add", "custom-repo", "http://example.com/repo", "-p", "150"]) == 0
    repos_data = config.load_json(config.repositories_json)
    assert repos_data["repositories"]["custom-repo"]["priority"] == 150

    # 2. List repos
    assert main(["repo", "list"]) == 0
    captured = capsys.readouterr()
    assert "custom-repo" in captured.out
    assert "150" in captured.out

    # 3. Modify repo priority with set-priority
    assert main(["repo", "set-priority", "custom-repo", "300"]) == 0
    repos_data = config.load_json(config.repositories_json)
    assert repos_data["repositories"]["custom-repo"]["priority"] == 300

    # 4. Set priority for non-existent repo should return error code
    assert main(["repo", "set-priority", "nonexistent-repo", "200"]) == 1
