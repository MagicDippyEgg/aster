"""
Local package registry and installed package metadata management.
"""

import time
from pathlib import Path
from typing import Dict, Any, List, Optional
from aster.config import AsterConfig
from aster.schema import validate_registry

class RegistryManager:
    def __init__(self, config: AsterConfig):
        self.config = config
        self.config.ensure_directories()
        self._ensure_registry_exists()

    def _ensure_registry_exists(self):
        if not self.config.packages_json.exists():
            initial_registry = {
                "schema_version": 1,
                "installed": {}
            }
            self.config.save_json_atomic(self.config.packages_json, initial_registry)

    def load_registry(self) -> Dict[str, Any]:
        """Loads and validates packages.json."""
        data = self.config.load_json(self.config.packages_json)
        if not data:
            data = {"schema_version": 1, "installed": {}}
            self.config.save_json_atomic(self.config.packages_json, data)
        else:
            validate_registry(data)
        return data

    def save_registry(self, registry_data: Dict[str, Any]):
        """Saves registry data atomically after validation."""
        validate_registry(registry_data)
        self.config.save_json_atomic(self.config.packages_json, registry_data)

    def is_installed(self, package_id: str) -> bool:
        """Checks whether a package ID is recorded as installed."""
        registry = self.load_registry()
        return package_id in registry.get("installed", {})

    def get_installed_package(self, package_id: str) -> Optional[Dict[str, Any]]:
        """Returns the registry entry for an installed package, if present."""
        registry = self.load_registry()
        return registry.get("installed", {}).get(package_id)

    def list_installed(self) -> Dict[str, Dict[str, Any]]:
        """Returns all installed package records."""
        registry = self.load_registry()
        return registry.get("installed", {})

    def register_package(
        self,
        package_id: str,
        name: str,
        version: str,
        pkg_type: str,
        installed_files: List[str],
        provided_binaries: List[str],
        source_repository: str = "default",
        extra_metadata: Optional[Dict[str, Any]] = None
    ):
        """
        Registers an installed package in `packages.json` and writes per-package `metadata.json`.
        """
        package_dir = self.config.packages_dir / package_id
        package_dir.mkdir(parents=True, exist_ok=True)

        metadata = {
            "schema_version": 1,
            "id": package_id,
            "name": name,
            "version": version,
            "type": pkg_type,
            "source_repository": source_repository,
            "installation_time": time.time(),
            "installed_files": installed_files,
            "provided_binaries": provided_binaries,
        }
        if extra_metadata:
            metadata.update(extra_metadata)

        metadata_file = package_dir / "metadata.json"
        self.config.save_json_atomic(metadata_file, metadata)

        registry = self.load_registry()
        registry["installed"][package_id] = {
            "name": name,
            "version": version,
            "type": pkg_type,
            "install_dir": str(package_dir.relative_to(self.config.root_dir)),
            "installed_files": installed_files,
            "provided_binaries": provided_binaries,
            "source_repository": source_repository,
        }
        self.save_registry(registry)

    def unregister_package(self, package_id: str):
        """
        Removes a package from `packages.json`.
        """
        registry = self.load_registry()
        if package_id in registry.get("installed", {}):
            del registry["installed"][package_id]
            self.save_registry(registry)
