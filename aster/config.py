"""
Configuration and directory management for Aster.
"""

import os
import json
import ssl
from pathlib import Path

DEFAULT_ASTER_HOME = Path.home() / ".bin" / "aster"

import sys
import urllib.request
import urllib.error

def fetch_url(url: str, headers: dict = None, timeout: int = 15) -> bytes:
    """
    Safely fetches bytes from a URL using SSL certificate verification with certifi
    support and unverified SSL fallback when local CA certificates are missing.
    """
    if headers is None:
        headers = {"User-Agent": "Aster-PackageManager/0.1.0"}

    req = urllib.request.Request(url, headers=headers)

    # Try with certifi or default context first
    try:
        import certifi
        ssl_ctx = ssl.create_default_context(cafile=certifi.where())
    except Exception:
        try:
            ssl_ctx = ssl.create_default_context()
        except Exception:
            ssl_ctx = ssl._create_unverified_context()

    try:
        with urllib.request.urlopen(req, timeout=timeout, context=ssl_ctx) as resp:
            return resp.read()
    except urllib.error.URLError as e:
        err_str = str(e)
        if hasattr(e, "reason"):
            err_str += f" {e.reason}"
        if "CERTIFICATE_VERIFY_FAILED" in err_str or "certificate verify failed" in err_str:
            sys.stderr.write(f"Warning: SSL certificate verification failed for {url}. Retrying with unverified SSL context...\n")
            unverified_ctx = ssl._create_unverified_context()
            with urllib.request.urlopen(req, timeout=timeout, context=unverified_ctx) as resp:
                return resp.read()
        raise e

def fetch_url_to_file(url: str, dest_path: Path, headers: dict = None, timeout: int = 30) -> None:
    """
    Downloads content from a URL directly to dest_path with SSL fallback.
    """
    data = fetch_url(url, headers=headers, timeout=timeout)
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    with open(dest_path, "wb") as f:
        f.write(data)

def get_clean_env() -> dict:
    """
    Returns a copy of os.environ with PyInstaller-injected library paths removed.
    This prevents subprocesses (like git, cmake, gcc) from failing due to OpenSSL/glibc
    version mismatches caused by PyInstaller's bundled shared libraries in LD_LIBRARY_PATH.
    """
    env = os.environ.copy()
    # Remove PyInstaller's injected LD_LIBRARY_PATH if present
    if "LD_LIBRARY_PATH_ORIG" in env:
        env["LD_LIBRARY_PATH"] = env["LD_LIBRARY_PATH_ORIG"]
        del env["LD_LIBRARY_PATH_ORIG"]
    elif "LD_LIBRARY_PATH" in env:
        # Check if LD_LIBRARY_PATH contains PyInstaller's _MEI temporary directory
        ld_paths = env["LD_LIBRARY_PATH"].split(os.pathsep)
        cleaned = [p for p in ld_paths if "_MEI" not in p]
        if cleaned:
            env["LD_LIBRARY_PATH"] = os.pathsep.join(cleaned)
        else:
            del env["LD_LIBRARY_PATH"]
    return env

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
