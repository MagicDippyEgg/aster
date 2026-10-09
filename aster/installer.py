"""
Installer for binary and source packages.
"""

import os
import shutil
import tarfile
import zipfile
import platform
import hashlib
import urllib.request
import urllib.error
import subprocess
from pathlib import Path
from typing import Dict, Any, List, Optional
from aster.config import AsterConfig
from aster.registry import RegistryManager
from aster.catalogue import CatalogueManager

def get_platform_key() -> str:
    """Detects platform string e.g. 'linux-x86_64' or 'linux-aarch64'."""
    sys_name = platform.system().lower()
    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        arch = "x86_64"
    elif machine in ("aarch64", "arm64"):
        arch = "aarch64"
    else:
        arch = machine
    return f"{sys_name}-{arch}"

class PackageInstaller:
    def __init__(self, config: AsterConfig, registry: RegistryManager, catalogue: CatalogueManager):
        self.config = config
        self.registry = registry
        self.catalogue = catalogue

    def install(self, package_id: str) -> None:
        """Installs a package and its dependencies by ID."""
        from aster.resolver import DependencyResolver, DependencyError
        resolver = DependencyResolver(self.catalogue, self.registry)

        try:
            install_plan = resolver.resolve_dependencies(package_id)
        except DependencyError as e:
            raise RuntimeError(f"Dependency resolution failed: {e}")

        for pkg_to_install in install_plan:
            if self.registry.is_installed(pkg_to_install):
                if pkg_to_install == package_id:
                    installed_pkg = self.registry.get_installed_package(package_id)
                    print(f"Package '{package_id}' is already installed (version {installed_pkg.get('version')}).")
                continue

            pkg_def = self.catalogue.get_package_definition(pkg_to_install)
            if not pkg_def:
                raise ValueError(f"Package definition for '{pkg_to_install}' not found in catalogue.")

            pkg_type = pkg_def.get("type")
            if pkg_type == "binary":
                self._install_binary(pkg_def)
            elif pkg_type == "source":
                self._install_source(pkg_def)
            else:
                raise ValueError(f"Unsupported package type '{pkg_type}' for package '{pkg_to_install}'.")

    def _check_binary_conflicts(self, provided_binaries: List[str], current_package_id: str):
        """Checks if provided binary links conflict with existing commands in bin_dir."""
        for binary_name in provided_binaries:
            target_link = self.config.bin_dir / binary_name
            if target_link.exists() or target_link.is_symlink():
                # Check if it belongs to another installed package
                installed_pkgs = self.registry.list_installed()
                for existing_id, existing_info in installed_pkgs.items():
                    if existing_id != current_package_id:
                        if binary_name in existing_info.get("provided_binaries", []):
                            raise RuntimeError(
                                f"Command conflict: '{binary_name}' is already provided by installed package '{existing_id}'."
                            )

    def _install_binary(self, pkg_def: dict) -> None:
        pkg_id = pkg_def["id"]
        version = pkg_def.get("version", "unknown")
        name = pkg_def.get("name", pkg_id)
        platform_key = get_platform_key()

        downloads = pkg_def.get("downloads", {})
        if platform_key not in downloads:
            # Fallback check for linux-x86_64
            supported = pkg_def.get("supported_platforms", list(downloads.keys()))
            raise RuntimeError(f"Platform '{platform_key}' is not supported by binary package '{pkg_id}'. Supported platforms: {supported}")

        dl_info = downloads[platform_key]
        url = dl_info.get("url")
        expected_sha256 = dl_info.get("sha256")

        print(f"Downloading binary release for {pkg_id} ({platform_key})...")
        download_filename = url.split("/")[-1] or f"{pkg_id}.archive"
        dest_archive = self.config.downloads_cache / download_filename

        if url.startswith("http://") or url.startswith("https://"):
            try:
                req = urllib.request.Request(url, headers={"User-Agent": "Aster-PackageManager/0.1.0"})
                with urllib.request.urlopen(req, timeout=30) as resp, open(dest_archive, "wb") as f:
                    shutil.copyfileobj(resp, f)
            except urllib.error.URLError as e:
                raise RuntimeError(f"Failed to download asset from {url}: {e}")
        elif url.startswith("file://"):
            src_path = Path(url[7:])
            if not src_path.exists():
                raise FileNotFoundError(f"Local binary asset not found: {src_path}")
            shutil.copy(src_path, dest_archive)
        else:
            src_path = Path(url)
            if not src_path.exists():
                raise FileNotFoundError(f"Local binary asset not found: {src_path}")
            shutil.copy(src_path, dest_archive)

        if expected_sha256 and expected_sha256 != "EXPECTED_SHA256":
            hasher = hashlib.sha256()
            with open(dest_archive, "rb") as f:
                while chunk := f.read(65536):
                    hasher.update(chunk)
            digest = hasher.hexdigest()
            if digest.lower() != expected_sha256.lower():
                raise RuntimeError(f"Integrity check failed for '{pkg_id}'. Expected sha256 {expected_sha256}, got {digest}")

        # Staging
        staging_dir = self.config.build_dir / f"staging-{pkg_id}"
        if staging_dir.exists():
            shutil.rmtree(staging_dir)
        staging_dir.mkdir(parents=True, exist_ok=True)

        print(f"Extracting release archive...")
        archive_format = pkg_def.get("archive", {}).get("format")
        if not archive_format:
            if download_filename.endswith(".tar.gz") or download_filename.endswith(".tgz"):
                archive_format = "tar.gz"
            elif download_filename.endswith(".zip"):
                archive_format = "zip"

        if str(dest_archive).endswith(".tar.gz") or str(dest_archive).endswith(".tgz") or archive_format == "tar.gz":
            with tarfile.open(dest_archive, "r:*") as tar:
                # Security path traversal check
                for member in tar.getmembers():
                    if member.name.startswith("/") or ".." in member.name:
                        raise RuntimeError(f"Unsafe file path in archive: {member.name}")
                tar.extractall(path=staging_dir)
        elif str(dest_archive).endswith(".zip") or archive_format == "zip":
            with zipfile.ZipFile(dest_archive, "r") as zip_ref:
                for name in zip_ref.namelist():
                    if name.startswith("/") or ".." in name:
                        raise RuntimeError(f"Unsafe file path in archive: {name}")
                zip_ref.extractall(path=staging_dir)
        else:
            # Single executable file or uncompressed binary
            dest_file = staging_dir / "bin" / pkg_id.replace("-bin", "")
            dest_file.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(dest_archive, dest_file)
            dest_file.chmod(0o755)

        # Locate binaries in staging
        extracted_bin_dir = staging_dir / "bin"
        if not extracted_bin_dir.exists():
            # Check if binaries are at root of staging
            executables = [f for f in staging_dir.iterdir() if f.is_file() and os.access(f, os.X_OK)]
            if executables:
                extracted_bin_dir = staging_dir / "bin"
                extracted_bin_dir.mkdir(parents=True, exist_ok=True)
                for exe in executables:
                    shutil.move(exe, extracted_bin_dir / exe.name)

        provided_binaries = []
        if extracted_bin_dir.exists():
            provided_binaries = [f.name for f in extracted_bin_dir.iterdir() if f.is_file()]

        if not provided_binaries:
            # Fallback to package executable name default
            default_bin_name = pkg_id.replace("-bin", "")
            provided_binaries = [default_bin_name]

        # Conflict check
        self._check_binary_conflicts(provided_binaries, pkg_id)

        # Move staging to package directory
        final_package_dir = self.config.packages_dir / pkg_id
        if final_package_dir.exists():
            shutil.rmtree(final_package_dir)
        shutil.move(staging_dir, final_package_dir)

        # Collect installed files relative to package dir
        installed_files = []
        for root, dirs, files in os.walk(final_package_dir):
            for file in files:
                full_p = Path(root) / file
                rel_p = full_p.relative_to(final_package_dir)
                installed_files.append(str(rel_p))

        # Create command symlinks in ~/.bin/aster/bin
        self.config.bin_dir.mkdir(parents=True, exist_ok=True)
        for bin_name in provided_binaries:
            target_bin = final_package_dir / "bin" / bin_name
            if not target_bin.exists():
                # If binary wasn't inside bin/, find it in final_package_dir
                for root, dirs, files in os.walk(final_package_dir):
                    if bin_name in files:
                        target_bin = Path(root) / bin_name
                        break

            if target_bin.exists():
                target_bin.chmod(0o755)
                link_path = self.config.bin_dir / bin_name
                if link_path.exists() or link_path.is_symlink():
                    link_path.unlink()
                link_path.symlink_to(target_bin)

        # Register package
        self.registry.register_package(
            package_id=pkg_id,
            name=name,
            version=version,
            pkg_type="binary",
            installed_files=installed_files,
            provided_binaries=provided_binaries,
        )
        print(f"Successfully installed '{pkg_id}' version {version}.")

    def _install_source(self, pkg_def: dict) -> None:
        pkg_id = pkg_def["id"]
        version = pkg_def.get("version", "unknown")
        name = pkg_def.get("name", pkg_id)

        source_info = pkg_def.get("source", {})
        src_type = source_info.get("type")
        src_url = source_info.get("url")

        print(f"Fetching source for {pkg_id}...")
        build_dir = self.config.build_dir / f"build-{pkg_id}"
        if build_dir.exists():
            shutil.rmtree(build_dir)
        build_dir.mkdir(parents=True, exist_ok=True)

        if src_type == "git":
            ref = source_info.get("ref", "main")
            cmd = ["git", "clone", "--depth", "1", "--branch", ref, src_url, str(build_dir)]
            res = subprocess.run(cmd, capture_output=True, text=True)
            if res.returncode != 0:
                # Try cloning default branch if branch fails
                cmd = ["git", "clone", "--depth", "1", src_url, str(build_dir)]
                res = subprocess.run(cmd, capture_output=True, text=True)
                if res.returncode != 0:
                    raise RuntimeError(f"Git clone failed for '{pkg_id}': {res.stderr}")
        elif src_type in ("tar.gz", "archive", "url"):
            dest_archive = self.config.downloads_cache / f"{pkg_id}.tar.gz"
            if src_url.startswith("http://") or src_url.startswith("https://"):
                req = urllib.request.Request(src_url, headers={"User-Agent": "Aster-PackageManager/0.1.0"})
                with urllib.request.urlopen(req, timeout=30) as resp, open(dest_archive, "wb") as f:
                    shutil.copyfileobj(resp, f)
            elif src_url.startswith("file://"):
                shutil.copy(Path(src_url[7:]), dest_archive)
            else:
                shutil.copy(Path(src_url), dest_archive)

            with tarfile.open(dest_archive, "r:*") as tar:
                for member in tar.getmembers():
                    if member.name.startswith("/") or ".." in member.name:
                        raise RuntimeError(f"Unsafe file path in archive: {member.name}")
                tar.extractall(path=build_dir)
        else:
            raise RuntimeError(f"Unsupported source type '{src_type}' for package '{pkg_id}'.")

        # Build steps
        build_info = pkg_def.get("build", {})
        build_system = build_info.get("system")
        staging_dir = self.config.build_dir / f"staging-{pkg_id}"
        if staging_dir.exists():
            shutil.rmtree(staging_dir)
        staging_dir.mkdir(parents=True, exist_ok=True)

        print(f"Building {pkg_id} ({build_system or 'custom'})...")
        if build_system == "cmake":
            cmake_build_dir = build_dir / "build_output"
            cmake_build_dir.mkdir(exist_ok=True)
            res = subprocess.run(
                ["cmake", "-B", str(cmake_build_dir), "-S", str(build_dir), f"-DCMAKE_INSTALL_PREFIX={staging_dir}"],
                capture_output=True, text=True
            )
            if res.returncode != 0:
                raise RuntimeError(f"CMake configuration failed: {res.stderr}")
            res = subprocess.run(["cmake", "--build", str(cmake_build_dir)], capture_output=True, text=True)
            if res.returncode != 0:
                raise RuntimeError(f"CMake build failed: {res.stderr}")
            res = subprocess.run(["cmake", "--install", str(cmake_build_dir)], capture_output=True, text=True)
            if res.returncode != 0:
                raise RuntimeError(f"CMake install failed: {res.stderr}")

        elif build_system == "make":
            res = subprocess.run(["make", "-C", str(build_dir)], capture_output=True, text=True)
            if res.returncode != 0:
                raise RuntimeError(f"Make build failed: {res.stderr}")
            res = subprocess.run(["make", "-C", str(build_dir), f"DESTDIR={staging_dir}", "install"], capture_output=True, text=True)
            if res.returncode != 0:
                # Try simple copy if make install fails or no install target
                pass

        elif build_info.get("steps"):
            for step in build_info.get("steps", []):
                res = subprocess.run(step, shell=True, cwd=str(build_dir), capture_output=True, text=True)
                if res.returncode != 0:
                    raise RuntimeError(f"Build step failed: '{step}': {res.stderr}")
        else:
            # Fallback search for built binaries or source files
            pass

        # Locate binaries in staging or build_dir
        extracted_bin_dir = staging_dir / "bin"
        if not extracted_bin_dir.exists():
            # Check build_dir for executables
            extracted_bin_dir = staging_dir / "bin"
            extracted_bin_dir.mkdir(parents=True, exist_ok=True)
            for root, dirs, files in os.walk(build_dir):
                for f in files:
                    fp = Path(root) / f
                    if fp.is_file() and os.access(fp, os.X_OK) and not f.endswith(".sh"):
                        shutil.copy(fp, extracted_bin_dir / f)

        provided_binaries = []
        if extracted_bin_dir.exists():
            provided_binaries = [f.name for f in extracted_bin_dir.iterdir() if f.is_file()]

        if not provided_binaries:
            provided_binaries = [pkg_id]

        self._check_binary_conflicts(provided_binaries, pkg_id)

        # Move staging to final package directory
        final_package_dir = self.config.packages_dir / pkg_id
        if final_package_dir.exists():
            shutil.rmtree(final_package_dir)
        shutil.move(staging_dir, final_package_dir)

        # Clean build directory
        if build_dir.exists():
            shutil.rmtree(build_dir)

        installed_files = []
        for root, dirs, files in os.walk(final_package_dir):
            for file in files:
                full_p = Path(root) / file
                rel_p = full_p.relative_to(final_package_dir)
                installed_files.append(str(rel_p))

        # Create symlinks
        self.config.bin_dir.mkdir(parents=True, exist_ok=True)
        for bin_name in provided_binaries:
            target_bin = final_package_dir / "bin" / bin_name
            if not target_bin.exists():
                for root, dirs, files in os.walk(final_package_dir):
                    if bin_name in files:
                        target_bin = Path(root) / bin_name
                        break

            if target_bin.exists():
                target_bin.chmod(0o755)
                link_path = self.config.bin_dir / bin_name
                if link_path.exists() or link_path.is_symlink():
                    link_path.unlink()
                link_path.symlink_to(target_bin)

        self.registry.register_package(
            package_id=pkg_id,
            name=name,
            version=version,
            pkg_type="source",
            installed_files=installed_files,
            provided_binaries=provided_binaries,
        )
        print(f"Successfully compiled and installed '{pkg_id}' version {version}.")

    def remove(self, package_id: str, force: bool = False) -> None:
        """Removes an installed package."""
        if not self.registry.is_installed(package_id):
            print(f"Package '{package_id}' is not installed.")
            return

        if not force:
            from aster.resolver import DependencyResolver
            resolver = DependencyResolver(self.catalogue, self.registry)
            dependents = resolver.check_removal_safety(package_id)
            if dependents:
                raise RuntimeError(
                    f"Cannot remove '{package_id}': required by installed package(s): {', '.join(dependents)}"
                )

        pkg_info = self.registry.get_installed_package(package_id)
        provided_binaries = pkg_info.get("provided_binaries", [])

        # Remove command symlinks
        for bin_name in provided_binaries:
            link_path = self.config.bin_dir / bin_name
            if link_path.is_symlink() or link_path.exists():
                try:
                    # Confirm symlink points to this package's folder before unlinking
                    target = link_path.resolve()
                    pkg_dir = self.config.packages_dir / package_id
                    if str(target).startswith(str(pkg_dir.resolve())) or not link_path.exists():
                        link_path.unlink()
                except Exception:
                    if link_path.is_symlink():
                        link_path.unlink()

        # Remove package directory
        pkg_dir = self.config.packages_dir / package_id
        if pkg_dir.exists():
            shutil.rmtree(pkg_dir)

        # Unregister from registry
        self.registry.unregister_package(package_id)
        print(f"Successfully removed '{package_id}'.")
