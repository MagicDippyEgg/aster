import json
import pytest
import shutil
import tempfile
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
