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

SYSTEM_CA_BUNDLES = [
    "/etc/ssl/certs/ca-certificates.crt",                  # Debian / Ubuntu / Gentoo
    "/etc/pki/tls/certs/ca-bundle.crt",                      # Fedora / RHEL / CentOS
    "/etc/ssl/ca-bundle.pem",                                # OpenSUSE
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",    # CentOS / RHEL 7+
    "/etc/ssl/cert.pem",                                     # Alpine / macOS / FreeBSD
]

def get_ssl_context() -> ssl.SSLContext:
    """
    Creates an SSL context with certificate verification support.
    Attempts:
    1. certifi package CA bundle (if available)
    2. Explicit system CA bundle paths on Linux/BSD/macOS
    3. Default SSL context
    """
    try:
        import certifi
        cafile = certifi.where()
        if os.path.exists(cafile):
            return ssl.create_default_context(cafile=cafile)
    except Exception:
        pass

    for cafile in SYSTEM_CA_BUNDLES:
        if os.path.exists(cafile):
            try:
                return ssl.create_default_context(cafile=cafile)
            except Exception:
                continue

    try:
        return ssl.create_default_context()
    except Exception:
        return ssl._create_unverified_context()

def fetch_url(url: str, headers: dict = None, timeout: int = 15) -> bytes:
    """
    Safely fetches bytes from a URL using SSL certificate verification.
    """
    if headers is None:
        headers = {"User-Agent": "Aster-PackageManager/0.1.2"}

    req = urllib.request.Request(url, headers=headers)
    ssl_ctx = get_ssl_context()

    try:
        with urllib.request.urlopen(req, timeout=timeout, context=ssl_ctx) as resp:
            return resp.read()
    except urllib.error.URLError as e:
        err_str = str(e)
        if hasattr(e, "reason"):
            err_str += f" {e.reason}"
        if "CERTIFICATE_VERIFY_FAILED" in err_str or "certificate verify failed" in err_str:
            # Attempt secondary fallback with explicit system CA bundle scan if first attempt failed
            for cafile in SYSTEM_CA_BUNDLES:
                if os.path.exists(cafile):
                    try:
                        alt_ctx = ssl.create_default_context(cafile=cafile)
                        with urllib.request.urlopen(req, timeout=timeout, context=alt_ctx) as resp:
                            return resp.read()
                    except Exception:
                        continue

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

def get_build_env(config: "AsterConfig", extra_prefix_dirs: list = None) -> dict:
    """
    Returns a build environment configured to search Aster-installed packages and optional
    temporary build dependency prefixes for include headers, libraries, binaries, and cmake/pkg-config files.
    """
    env = get_clean_env()

    prefix_dirs = []
    if extra_prefix_dirs:
        prefix_dirs.extend([Path(p) for p in extra_prefix_dirs])

    # Scan installed package directories under config.packages_dir
    if config and config.packages_dir.exists():
        for pkg_dir in config.packages_dir.iterdir():
            if pkg_dir.is_dir():
                prefix_dirs.append(pkg_dir)

    bin_paths = []
    inc_paths = []
    lib_paths = []
    cmake_paths = []
    pkgconfig_paths = []

    for p in prefix_dirs:
        # Binaries
        b_dir = p / "bin"
        if b_dir.exists():
            bin_paths.append(str(b_dir))

        # Include headers
        for inc in [p / "include", p / "usr" / "include"]:
            if inc.exists():
                inc_paths.append(str(inc))

        # Libraries
        for lib in [p / "lib", p / "lib64", p / "usr" / "lib", p / "usr" / "lib64"]:
            if lib.exists():
                lib_paths.append(str(lib))

        # CMake prefix
        cmake_paths.append(str(p))

        # pkg-config
        for pc in [p / "lib" / "pkgconfig", p / "lib64" / "pkgconfig", p / "share" / "pkgconfig"]:
            if pc.exists():
                pkgconfig_paths.append(str(pc))

    # Prepend to environment variables
    if bin_paths:
        current_path = env.get("PATH", "")
        env["PATH"] = os.pathsep.join(bin_paths + [current_path] if current_path else bin_paths)

    if inc_paths:
        inc_str = os.pathsep.join(inc_paths)
        for var in ["CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH"]:
            curr = env.get(var, "")
            env[var] = os.pathsep.join([inc_str, curr]) if curr else inc_str

    if lib_paths:
        lib_str = os.pathsep.join(lib_paths)
        for var in ["LIBRARY_PATH", "LD_LIBRARY_PATH"]:
            curr = env.get(var, "")
            env[var] = os.pathsep.join([lib_str, curr]) if curr else lib_str

    if cmake_paths:
        cmake_str = os.pathsep.join(cmake_paths)
        curr = env.get("CMAKE_PREFIX_PATH", "")
        env["CMAKE_PREFIX_PATH"] = os.pathsep.join([cmake_str, curr]) if curr else cmake_str

    if pkgconfig_paths:
        pc_str = os.pathsep.join(pkgconfig_paths)
        curr = env.get("PKG_CONFIG_PATH", "")
        env["PKG_CONFIG_PATH"] = os.pathsep.join([pc_str, curr]) if curr else pc_str

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
        self.toolchains_dir = self.cache_dir / "toolchains"
        self.rust_toolchain_dir = self.toolchains_dir / "rust"

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
