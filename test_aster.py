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
from aster.cli import main
from aster.schema import ValidationError, validate_package_definition, validate_index, validate_registry

@pytest.fixture
def temp_aster_env(monkeypatch):
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_path = Path(tmpdir)
        monkeypatch.setenv("ASTER_HOME", str(tmp_path / "aster"))

        # Create a local test catalogue
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

    # 9. Remove package
    assert main(["remove", "fastfetch-bin"]) == 0
    assert not bin_link.exists()
    assert main(["installed", "fastfetch-bin"]) == 1

def test_install_source_package(temp_aster_env):
    config = temp_aster_env
    catalogue = CatalogueManager(config)
    catalogue.update_all()

    assert main(["install", "test-src"]) == 0
    assert (config.bin_dir / "test-src").exists()
    assert main(["remove", "test-src"]) == 0
    assert not (config.bin_dir / "test-src").exists()
