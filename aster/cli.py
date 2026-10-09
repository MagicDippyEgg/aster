"""
Command Line Interface for Aster Package Manager.
"""

import sys
import argparse
from typing import List, Optional
from aster import __version__
from aster.config import AsterConfig
from aster.registry import RegistryManager
from aster.catalogue import CatalogueManager
from aster.installer import PackageInstaller

def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="aster",
        description="Aster Package Manager - Independent Linux Package Manager",
        add_help=True
    )
    parser.add_argument("--version", action="version", version=f"aster {__version__}")

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # aster help
    subparsers.add_parser("help", help="Show help message")

    # aster list
    subparsers.add_parser("list", help="List installed packages")

    # aster search <query>
    search_parser = subparsers.add_parser("search", help="Search remote package catalogues")
    search_parser.add_argument("query", nargs="?", default="", help="Search query")

    # aster info <package>
    info_parser = subparsers.add_parser("info", help="Show metadata for a package")
    info_parser.add_argument("package", help="Package ID")

    # aster installed <package>
    installed_parser = subparsers.add_parser("installed", help="Check if package is installed")
    installed_parser.add_argument("package", help="Package ID")

    # aster install <package>
    install_parser = subparsers.add_parser("install", help="Install a package")
    install_parser.add_argument("package", help="Package ID")
    install_parser.add_argument("-y", "--yes", action="store_true", help="Automatically confirm custom build steps and prompts")

    # aster remove <package>
    remove_parser = subparsers.add_parser("remove", help="Remove an installed package")
    remove_parser.add_argument("package", help="Package ID")
    remove_parser.add_argument("--force", action="store_true", help="Force removal ignoring dependencies")

    # aster clean
    subparsers.add_parser("clean", help="Clear temporary build and download caches")

    # aster doctor
    subparsers.add_parser("doctor", help="Diagnose installation, path, and tool issues")

    # aster history
    subparsers.add_parser("history", help="Show history log of package operations")

    # aster update
    subparsers.add_parser("update", help="Refresh package indexes and show available updates")

    # aster upgrade
    upgrade_parser = subparsers.add_parser("upgrade", help="Upgrade installed packages")
    upgrade_parser.add_argument("package", nargs="?", default="", help="Optional package ID to upgrade")

    # aster repo <subcommand>
    repo_parser = subparsers.add_parser("repo", help="Manage package repositories")
    repo_subparsers = repo_parser.add_subparsers(dest="repo_command")
    repo_subparsers.add_parser("list", help="List configured repositories")
    repo_subparsers.add_parser("update", help="Update cached package indexes")

    repo_add = repo_subparsers.add_parser("add", help="Add a package repository")
    repo_add.add_argument("name", help="Repository name")
    repo_add.add_argument("url", help="Repository URL or path")

    repo_remove = repo_subparsers.add_parser("remove", help="Remove a package repository")
    repo_remove.add_argument("name", help="Repository name")

    return parser

def main(args: Optional[List[str]] = None) -> int:
    parser = create_parser()
    parsed_args = parser.parse_args(args)

    if not parsed_args.command or parsed_args.command == "help":
        parser.print_help()
        return 0

    config = AsterConfig()
    registry = RegistryManager(config)
    catalogue = CatalogueManager(config)
    installer = PackageInstaller(config, registry, catalogue)

    try:
        if parsed_args.command == "list":
            installed = registry.list_installed()
            if not installed:
                print("No packages currently installed.")
            else:
                print(f"{'PACKAGE ID':<20} {'VERSION':<15} {'TYPE':<10} {'REPOSITORY'}")
                print("-" * 60)
                for pkg_id, info in installed.items():
                    print(f"{pkg_id:<20} {info.get('version', 'n/a'):<15} {info.get('type', 'n/a'):<10} {info.get('source_repository', 'default')}")
            return 0

        elif parsed_args.command == "search":
            query = parsed_args.query
            results = catalogue.search_packages(query)
            if not results:
                print(f"No packages found matching '{query}'.")
            else:
                print(f"{'PACKAGE ID':<20} {'TYPE':<10} {'DESCRIPTION'}")
                print("-" * 65)
                for pkg_id, info in results.items():
                    desc = info.get("description", "")
                    pkg_type = info.get("type", "")
                    print(f"{pkg_id:<20} {pkg_type:<10} {desc}")
            return 0

        elif parsed_args.command == "info":
            pkg_id = parsed_args.package
            # Check local installed first or remote definition
            installed_info = registry.get_installed_package(pkg_id)
            pkg_def = catalogue.get_package_definition(pkg_id)

            if not installed_info and not pkg_def:
                print(f"Package '{pkg_id}' not found in registry or catalogue.")
                return 1

            print(f"Package: {pkg_id}")
            if installed_info:
                print(f"  Installed Version: {installed_info.get('version')}")
                print(f"  Installation Dir:  {installed_info.get('install_dir')}")
            if pkg_def:
                print(f"  Name:        {pkg_def.get('name')}")
                print(f"  Version:     {pkg_def.get('version')}")
                print(f"  Type:        {pkg_def.get('type')}")
                print(f"  Description: {pkg_def.get('description', 'N/A')}")
                print(f"  Homepage:    {pkg_def.get('homepage', 'N/A')}")
            return 0

        elif parsed_args.command == "installed":
            pkg_id = parsed_args.package
            if registry.is_installed(pkg_id):
                info = registry.get_installed_package(pkg_id)
                print(f"{pkg_id} is installed (version {info.get('version')}).")
                return 0
            else:
                print(f"{pkg_id} is not installed.")
                return 1

        elif parsed_args.command == "install":
            pkg_id = parsed_args.package
            auto_yes = getattr(parsed_args, "yes", False)
            installer.install(pkg_id, auto_yes=auto_yes)
            return 0

        elif parsed_args.command == "remove":
            pkg_id = parsed_args.package
            force = getattr(parsed_args, "force", False)
            installer.remove(pkg_id, force=force)
            return 0

        elif parsed_args.command == "clean":
            import shutil
            downloads = config.downloads_cache
            archives = config.archives_cache
            build = config.build_dir
            cleaned_size = 0
            for path in [downloads, archives, build]:
                if path.exists():
                    for item in path.iterdir():
                        if item.is_file():
                            cleaned_size += item.stat().st_size
                            item.unlink()
                        elif item.is_dir():
                            shutil.rmtree(item)
            print("Successfully cleared build and download caches.")
            return 0

        elif parsed_args.command == "doctor":
            import os, shutil
            print("Aster Doctor - Diagnostic Check")
            print("=" * 40)
            print(f"Aster Home: {config.root_dir}")

            # Check PATH
            path_env = os.environ.get("PATH", "")
            aster_bin_str = str(config.bin_dir)
            aster_home_bin_str = str(config.root_dir)

            bin_in_path = aster_bin_str in path_env
            home_bin_in_path = aster_home_bin_str in path_env

            print(f"Directory {config.bin_dir} in PATH: {'YES' if bin_in_path else 'NO'}")
            if not bin_in_path:
                print(f"  Warning: Add '{config.bin_dir}' to your PATH to run installed commands.")

            print(f"Directory {config.root_dir} in PATH: {'YES' if home_bin_in_path else 'NO'}")
            if not home_bin_in_path:
                print(f"  Warning: Add '{config.root_dir}' to your PATH to run 'aster' executable.")

            # System tools check
            tools = ["git", "cmake", "make", "gcc", "tar", "unzip"]
            print("\nBuild Tools Availability:")
            for tool in tools:
                found = shutil.which(tool)
                print(f"  {tool:<10}: {'FOUND (' + found + ')' if found else 'NOT FOUND'}")

            return 0

        elif parsed_args.command == "history":
            history_file = config.logs_dir / "history.log"
            if not history_file.exists():
                print("No history recorded yet.")
            else:
                with open(history_file, "r", encoding="utf-8") as f:
                    print(f.read())
            return 0

        elif parsed_args.command == "update":
            print("Refreshing package indexes...")
            catalogue.update_all()
            installed = registry.list_installed()
            updates_found = []
            from aster.version import compare_versions
            for pkg_id, inst_info in installed.items():
                pkg_def = catalogue.get_package_definition(pkg_id)
                if pkg_def:
                    avail_ver = pkg_def.get("version", "")
                    inst_ver = inst_info.get("version", "")
                    if compare_versions(inst_ver, avail_ver) < 0:
                        updates_found.append((pkg_id, inst_ver, avail_ver))

            if updates_found:
                print("\nAvailable package updates:")
                print(f"{'PACKAGE ID':<20} {'INSTALLED':<15} {'AVAILABLE'}")
                print("-" * 50)
                for pkg_id, inst_v, avail_v in updates_found:
                    print(f"{pkg_id:<20} {inst_v:<15} {avail_v}")
                print("\nRun 'aster upgrade' to upgrade all packages.")
            else:
                print("All installed packages are up to date.")
            return 0

        elif parsed_args.command == "upgrade":
            target_pkg = parsed_args.package
            catalogue.update_all()
            from aster.version import compare_versions

            if target_pkg:
                if not registry.is_installed(target_pkg):
                    print(f"Package '{target_pkg}' is not installed.")
                    return 1
                inst_info = registry.get_installed_package(target_pkg)
                pkg_def = catalogue.get_package_definition(target_pkg)
                if not pkg_def:
                    print(f"No definition found for '{target_pkg}' in catalogue.")
                    return 1
                inst_ver = inst_info.get("version", "")
                avail_ver = pkg_def.get("version", "")
                if compare_versions(inst_ver, avail_ver) < 0:
                    print(f"Upgrading '{target_pkg}' from {inst_ver} to {avail_ver}...")
                    installer.remove(target_pkg)
                    installer.install(target_pkg)
                else:
                    print(f"Package '{target_pkg}' is already at latest version ({inst_ver}).")
            else:
                installed = registry.list_installed()
                upgraded_any = False
                for pkg_id, inst_info in list(installed.items()):
                    pkg_def = catalogue.get_package_definition(pkg_id)
                    if pkg_def:
                        inst_ver = inst_info.get("version", "")
                        avail_ver = pkg_def.get("version", "")
                        if compare_versions(inst_ver, avail_ver) < 0:
                            print(f"Upgrading '{pkg_id}' from {inst_ver} to {avail_ver}...")
                            installer.remove(pkg_id)
                            installer.install(pkg_id)
                            upgraded_any = True
                if not upgraded_any:
                    print("All packages are already up to date.")
            return 0

        elif parsed_args.command == "repo":
            repos_data = config.load_json(config.repositories_json)
            repos = repos_data.get("repositories", {})

            if parsed_args.repo_command == "list":
                print(f"{'NAME':<15} {'URL'}")
                print("-" * 50)
                for name, info in repos.items():
                    print(f"{name:<15} {info.get('url')}")
                return 0
            elif parsed_args.repo_command == "add":
                r_name = parsed_args.name
                r_url = parsed_args.url
                repos[r_name] = {"name": r_name, "url": r_url}
                repos_data["repositories"] = repos
                config.save_json_atomic(config.repositories_json, repos_data)
                print(f"Repository '{r_name}' added successfully.")
                return 0
            elif parsed_args.repo_command == "remove":
                r_name = parsed_args.name
                if r_name in repos:
                    del repos[r_name]
                    repos_data["repositories"] = repos
                    config.save_json_atomic(config.repositories_json, repos_data)
                    print(f"Repository '{r_name}' removed successfully.")
                else:
                    print(f"Repository '{r_name}' not found.")
                return 0
            elif parsed_args.repo_command == "update":
                print("Updating package catalogue indexes...")
                catalogue.update_all()
                print("Catalogue update complete.")
                return 0
            else:
                parser.parse_args(["repo", "--help"])
                return 0

    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    return 0

if __name__ == "__main__":
    sys.exit(main())
