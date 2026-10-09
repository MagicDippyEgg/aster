"""
Configuration and directory management for Aster.
"""

import os
import json
from pathlib import Path

DEFAULT_ASTER_HOME = Path.home() / ".bin" / "aster"

class AsterConfig:
    def __init__(self, root_dir: Path = None):
        if root_dir:
            self.root_dir = Path(root_dir).expanduser().resolve()
        elif "ASTER_HOME" in os.environ:
            self.root_dir = Path(os.environ["ASTER_HOME"]).expanduser().resolve()
        else:
            self.root_dir = DEFAULT_ASTER_HOME

        self.aster_bin = self.root_dir / "aster"
        self.packages_json = self.root_dir / "packages.json"
        self.config_json = self.root_dir / "config.json"
        self.repositories_json = self.root_dir / "repositories.json"
        self.bin_dir = self.root_dir / "bin"
        self.packages_dir = self.root_dir / "packages"
        self.cache_dir = self.root_dir / "cache"
        self.downloads_cache = self.cache_dir / "downloads"
        self.archives_cache = self.cache_dir / "archives"
        self.build_dir = self.root_dir / "build"
        self.repository_cache = self.root_dir / "repository-cache"
        self.logs_dir = self.root_dir / "logs"

    def ensure_directories(self):
        """Creates all required directories if they don't exist."""
        directories = [
            self.root_dir,
            self.bin_dir,
            self.packages_dir,
            self.cache_dir,
            self.downloads_cache,
            self.archives_cache,
            self.build_dir,
            self.repository_cache,
            self.logs_dir,
        ]
        for d in directories:
            d.mkdir(parents=True, exist_ok=True)

        if not self.config_json.exists():
            default_config = {
                "schema_version": 1,
                "default_repository": "default",
            }
            self.save_json_atomic(self.config_json, default_config)

        if not self.repositories_json.exists():
            default_repos = {
                "schema_version": 1,
                "repositories": {
                    "default": {
                        "name": "default",
                        "url": "https://raw.githubusercontent.com/MagicDippyEgg/aster-package-repository/main"
                    }
                }
            }
            self.save_json_atomic(self.repositories_json, default_repos)

    @staticmethod
    def save_json_atomic(path: Path, data: dict):
        """Atomically save data as JSON to path."""
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = path.with_suffix(".tmp." + str(os.getpid()))
        with open(tmp_path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=4)
        tmp_path.replace(path)

    @staticmethod
    def load_json(path: Path) -> dict:
        """Safely load JSON data from path."""
        if not path.exists():
            return {}
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
