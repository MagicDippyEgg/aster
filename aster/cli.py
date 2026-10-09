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

    # aster remove <package>
    remove_parser = subparsers.add_parser("remove", help="Remove an installed package")
    remove_parser.add_argument("package", help="Package ID")

    # aster repo <subcommand>
    repo_parser = subparsers.add_parser("repo", help="Manage package repositories")
    repo_subparsers = repo_parser.add_subparsers(dest="repo_command")
    repo_subparsers.add_parser("list", help="List configured repositories")
    repo_subparsers.add_parser("update", help="Update cached package indexes")

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
            installer.install(pkg_id)
            return 0

        elif parsed_args.command == "remove":
            pkg_id = parsed_args.package
            installer.remove(pkg_id)
            return 0

        elif parsed_args.command == "repo":
            if parsed_args.repo_command == "list":
                repos = catalogue.get_repositories()
                print(f"{'NAME':<15} {'URL'}")
                print("-" * 50)
                for name, info in repos.items():
                    print(f"{name:<15} {info.get('url')}")
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
