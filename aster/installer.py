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
from aster.config import AsterConfig, fetch_url_to_file, get_clean_env, get_build_env
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

    def install(self, package_id: str, auto_yes: bool = False) -> None:
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
                self._install_source(pkg_def, auto_yes=auto_yes)
            else:
                raise ValueError(f"Unsupported package type '{pkg_type}' for package '{pkg_to_install}'.")

    def _handle_build_failure(self, output: str, stage_name: str) -> None:
        """Parses build error log output and prints actionable diagnostic details."""
        import re
        missing_headers = re.findall(r"fatal error:\s*([^\s:]+\.h):\s*No such file or directory", output)
        missing_pkgconfigs = re.findall(r"Could NOT find ([^\s]+)\s+\(missing:", output, re.IGNORECASE) or \
                             re.findall(r"Package ['\"]?([^'\"\s]+)['\"]? not found", output)

        msg = [f"{stage_name} failed.\n"]
        if missing_headers or missing_pkgconfigs:
            msg.append("DIAGNOSTIC HINT: Missing system compilation prerequisites detected.")
            if missing_headers:
                unique_headers = sorted(list(set(missing_headers)))
                msg.append(f"  Missing Header(s): {', '.join(unique_headers)}")
                # Known package mapping hints
                hints = []
                for header in unique_headers:
                    if "vulkan" in header:
                        hints.append("vulkan development package (e.g., 'libvulkan-dev' on Ubuntu/Debian, 'vulkan-headers' on Fedora/Arch)")
                    elif "wayland" in header:
                        hints.append("wayland development package (e.g., 'libwayland-dev' or 'wayland-protocols')")
                    elif "x11" in header or "X11" in header:
                        hints.append("X11 development package (e.g., 'libx11-dev' or 'libxcb1-dev')")
                    elif "pci" in header:
                        hints.append("pciutils development package (e.g., 'libpci-dev' or 'pciutils-devel')")
                if hints:
                    msg.append("  Suggested System Packages: " + "; ".join(hints))

            if missing_pkgconfigs:
                unique_pkgs = sorted(list(set(missing_pkgconfigs)))
                msg.append(f"  Missing Library/Module(s): {', '.join(unique_pkgs)}")

            msg.append("\nNote: Aster does not manage or automatically install system C/C++ development libraries.")
            msg.append("Please install the required system development header packages using your Linux distribution's package manager.")
        else:
            # Print excerpt of stderr if no specific missing headers identified
            lines = [l for l in output.splitlines() if "error:" in l or "Error" in l or "fatal:" in l]
            if lines:
                msg.append("Error excerpt:")
                msg.extend(lines[:10])

        raise RuntimeError("\n".join(msg))

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
                fetch_url_to_file(url, dest_archive, timeout=30)
            except Exception as e:
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

        # Un-nest single top-level directory if archive extracted into a nested directory e.g. ripgrep-14.1.0-x86_64-unknown-linux-musl/
        extracted_items = [p for p in staging_dir.iterdir()]
        if len(extracted_items) == 1 and extracted_items[0].is_dir():
            nested_dir = extracted_items[0]
            for item in nested_dir.iterdir():
                shutil.move(str(item), str(staging_dir / item.name))
            shutil.rmtree(str(nested_dir))

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

    def _install_source(self, pkg_def: dict, auto_yes: bool = False) -> None:
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

        build_env = get_build_env(self.config)

        if src_type == "git":
            ref = source_info.get("ref", "main")
            cmd = ["git", "clone", "--depth", "1", "--branch", ref, src_url, str(build_dir)]
            res = subprocess.run(cmd, capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                # Try cloning default branch if branch fails
                cmd = ["git", "clone", "--depth", "1", src_url, str(build_dir)]
                res = subprocess.run(cmd, capture_output=True, text=True, env=build_env)
                if res.returncode != 0:
                    raise RuntimeError(f"Git clone failed for '{pkg_id}': {res.stderr}")
        elif src_type in ("tar.gz", "zip", "archive", "url"):
            dest_archive = self.config.downloads_cache / (f"{pkg_id}.zip" if src_type == "zip" or src_url.endswith(".zip") else f"{pkg_id}.tar.gz")
            if src_url.startswith("http://") or src_url.startswith("https://"):
                fetch_url_to_file(src_url, dest_archive, timeout=30)
            elif src_url.startswith("file://"):
                shutil.copy(Path(src_url[7:]), dest_archive)
            else:
                shutil.copy(Path(src_url), dest_archive)

            if str(dest_archive).endswith(".zip") or src_type == "zip":
                with zipfile.ZipFile(dest_archive, "r") as zip_ref:
                    for name in zip_ref.namelist():
                        if name.startswith("/") or ".." in name:
                            raise RuntimeError(f"Unsafe file path in archive: {name}")
                    zip_ref.extractall(path=build_dir)
            else:
                with tarfile.open(dest_archive, "r:*") as tar:
                    for member in tar.getmembers():
                        if member.name.startswith("/") or ".." in member.name:
                            raise RuntimeError(f"Unsafe file path in archive: {member.name}")
                    tar.extractall(path=build_dir)
        else:
            raise RuntimeError(f"Unsupported source type '{src_type}' for package '{pkg_id}'.")

        # If single top-level directory extracted, un-nest build_dir
        extracted_items = [p for p in build_dir.iterdir() if p.name not in (".git",)]
        if len(extracted_items) == 1 and extracted_items[0].is_dir():
            nested_dir = extracted_items[0]
            if not (build_dir / "CMakeLists.txt").exists() and not (build_dir / "Makefile").exists():
                for item in nested_dir.iterdir():
                    shutil.move(str(item), str(build_dir / item.name))
                shutil.rmtree(str(nested_dir))

        # Build steps
        build_info = pkg_def.get("build", {})
        build_system = build_info.get("system")
        build_steps = build_info.get("steps")
        staging_dir = self.config.build_dir / f"staging-{pkg_id}"
        if staging_dir.exists():
            shutil.rmtree(staging_dir)
        staging_dir.mkdir(parents=True, exist_ok=True)

        if build_steps:
            steps = build_steps
            print(f"\nPackage '{pkg_id}' defines custom build steps:")
            for idx, step in enumerate(steps, 1):
                print(f"  {idx}. {step}")

            if not auto_yes:
                try:
                    response = input("\nDo you want to execute these build steps? [y/N]: ").strip().lower()
                except (EOFError, KeyboardInterrupt):
                    response = "n"
                if response not in ("y", "yes"):
                    print("Installation cancelled by user.")
                    if build_dir.exists():
                        shutil.rmtree(build_dir)
                    if staging_dir.exists():
                        shutil.rmtree(staging_dir)
                    return

            for step in steps:
                print(f"Executing step: {step}")
                res = subprocess.run(step, shell=True, cwd=str(build_dir), capture_output=True, text=True, env=build_env)
                if res.returncode != 0:
                    self._handle_build_failure(res.stdout + "\n" + res.stderr, f"Build step '{step}'")

        elif build_system == "cargo":
            print(f"Building {pkg_id} (cargo)...")
            cargo_bin = shutil.which("cargo", path=build_env.get("PATH"))
            rust_toolchain_bin = self.config.rust_toolchain_dir / "bin"
            if not cargo_bin and (rust_toolchain_bin / "cargo").exists():
                cargo_bin = str(rust_toolchain_bin / "cargo")

            downloaded_toolchain = False
            if not cargo_bin:
                print(f"\nAster: Cargo is required to build {pkg_id}.")
                print("       Rust is not installed in Aster's build environment.")
                print("       Downloading an isolated Rust toolchain...")

                self.config.rust_toolchain_dir.mkdir(parents=True, exist_ok=True)
                rustup_init_path = self.config.downloads_cache / "rustup-init"
                arch_key = platform.machine().lower()
                rustup_arch = "x86_64" if arch_key in ("x86_64", "amd64") else ("aarch64" if arch_key in ("aarch64", "arm64") else arch_key)
                rustup_url = f"https://static.rust-lang.org/rustup/dist/{rustup_arch}-unknown-linux-gnu/rustup-init"

                try:
                    fetch_url_to_file(rustup_url, rustup_init_path, timeout=60)
                    rustup_init_path.chmod(0o755)

                    tc_env = build_env.copy()
                    tc_env["RUSTUP_HOME"] = str(self.config.rust_toolchain_dir)
                    tc_env["CARGO_HOME"] = str(self.config.rust_toolchain_dir)

                    res = subprocess.run(
                        [str(rustup_init_path), "-y", "--no-modify-path", "--profile", "minimal"],
                        capture_output=True, text=True, env=tc_env
                    )
                    if res.returncode != 0:
                        raise RuntimeError(f"Failed to bootstrap isolated Rust toolchain: {res.stderr}")

                    cargo_bin = str(rust_toolchain_bin / "cargo")
                    downloaded_toolchain = True
                except Exception as e:
                    if downloaded_toolchain and self.config.rust_toolchain_dir.exists():
                        shutil.rmtree(self.config.rust_toolchain_dir)
                    raise RuntimeError(f"Failed to install isolated Rust toolchain: {e}")

            build_env["RUSTUP_HOME"] = str(self.config.rust_toolchain_dir)
            build_env["CARGO_HOME"] = str(self.config.rust_toolchain_dir)
            build_env["PATH"] = os.pathsep.join([str(rust_toolchain_bin), build_env.get("PATH", "")])

            cargo_cmd = [cargo_bin, "build"]
            if build_info.get("release", True):
                cargo_cmd.append("--release")
            if build_info.get("locked", False):
                cargo_cmd.append("--locked")

            res = subprocess.run(cargo_cmd, cwd=str(build_dir), capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                if downloaded_toolchain and self.config.load_json(self.config.config_json).get("cache_rust_toolchain") is False:
                    shutil.rmtree(self.config.rust_toolchain_dir, ignore_errors=True)
                self._handle_build_failure(res.stdout + "\n" + res.stderr, "Cargo build")

            # Stage declared executables or find built binaries in target/release
            target_profile = "release" if build_info.get("release", True) else "debug"
            target_dir = build_dir / "target" / target_profile

            declared_execs = pkg_def.get("executables") or build_info.get("executables") or []
            if isinstance(declared_execs, str):
                declared_execs = [declared_execs]

            staging_bin = staging_dir / "bin"
            staging_bin.mkdir(parents=True, exist_ok=True)

            if declared_execs:
                for exe_name in declared_execs:
                    built_exe = target_dir / exe_name
                    if built_exe.exists():
                        shutil.copy(built_exe, staging_bin / exe_name)
                        (staging_bin / exe_name).chmod(0o755)
            else:
                # Copy executables from target_dir
                if target_dir.exists():
                    for item in target_dir.iterdir():
                        if item.is_file() and os.access(item, os.X_OK) and not item.name.startswith("."):
                            shutil.copy(item, staging_bin / item.name)
                            (staging_bin / item.name).chmod(0o755)

            # Caching preference prompt if downloaded
            if downloaded_toolchain or (rust_toolchain_bin / "cargo").exists():
                cfg_data = self.config.load_json(self.config.config_json)
                cache_pref = cfg_data.get("cache_rust_toolchain")

                if cache_pref is None:
                    print("\nKeep the downloaded Rust toolchain and")
                    print("Cargo dependencies cached for future installs?")
                    if auto_yes:
                        choice = True
                    else:
                        try:
                            resp = input("[Y/n]: ").strip().lower()
                            choice = resp in ("", "y", "yes")
                        except (EOFError, KeyboardInterrupt):
                            choice = True

                    cfg_data["cache_rust_toolchain"] = choice
                    self.config.save_json_atomic(self.config.config_json, cfg_data)
                    print(f"\nPreference saved: cache_rust_toolchain = {choice}")
                    print("You can change this setting anytime using: aster config cache-rust <true|false>\n")
                    cache_pref = choice

                if cache_pref is False and self.config.rust_toolchain_dir.exists():
                    shutil.rmtree(self.config.rust_toolchain_dir, ignore_errors=True)

        elif build_system == "cmake":
            print(f"Building {pkg_id} (cmake)...")
            cmake_build_dir = build_dir / "build_output"
            cmake_build_dir.mkdir(exist_ok=True)
            res = subprocess.run(
                ["cmake", "-B", str(cmake_build_dir), "-S", str(build_dir), f"-DCMAKE_INSTALL_PREFIX={staging_dir}"],
                capture_output=True, text=True, env=build_env
            )
            if res.returncode != 0:
                self._handle_build_failure(res.stdout + "\n" + res.stderr, "CMake configuration")
            res = subprocess.run(["cmake", "--build", str(cmake_build_dir)], capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                self._handle_build_failure(res.stdout + "\n" + res.stderr, "CMake build")
            res = subprocess.run(["cmake", "--install", str(cmake_build_dir)], capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                self._handle_build_failure(res.stdout + "\n" + res.stderr, "CMake install")

        elif build_system == "make":
            print(f"Building {pkg_id} (make)...")
            res = subprocess.run(["make", "-C", str(build_dir)], capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                self._handle_build_failure(res.stdout + "\n" + res.stderr, "Make build")
            res = subprocess.run(["make", "-C", str(build_dir), f"DESTDIR={staging_dir}", "install"], capture_output=True, text=True, env=build_env)
            if res.returncode != 0:
                # Try simple copy if make install fails or no install target
                pass

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
