"""
Package dependency resolution and cycle detection module for Aster.
"""

from typing import Dict, List, Set, Tuple, Any, Optional

class DependencyError(Exception):
    pass

class DependencyResolver:
    def __init__(self, catalogue, registry):
        self.catalogue = catalogue
        self.registry = registry

    def resolve_dependencies(self, package_id: str) -> List[str]:
        """
        Resolves package dependencies recursively.
        Returns an ordered list of package IDs to install (dependencies first, package_id last).
        Detects circular dependencies and raises DependencyError if found.
        """
        install_order: List[str] = []
        visited: Set[str] = set()
        in_stack: Set[str] = set()

        def visit(pkg_id: str, path: List[str]):
            if pkg_id in in_stack:
                cycle = " -> ".join(path + [pkg_id])
                raise DependencyError(f"Circular dependency detected: {cycle}")
            if pkg_id in visited:
                return

            in_stack.add(pkg_id)
            pkg_def = self.catalogue.get_package_definition(pkg_id)
            if not pkg_def:
                raise DependencyError(f"Dependency package definition '{pkg_id}' not found in catalogue.")

            # Dependencies can be under 'dependencies' array or 'build.dependencies' / 'runtime.dependencies'
            deps = []
            if "dependencies" in pkg_def and isinstance(pkg_def["dependencies"], list):
                deps.extend(pkg_def["dependencies"])
            if "build" in pkg_def and isinstance(pkg_def["build"], dict):
                build_deps = pkg_def["build"].get("dependencies", [])
                if isinstance(build_deps, list):
                    deps.extend(build_deps)
            if "runtime" in pkg_def and isinstance(pkg_def["runtime"], dict):
                rt_deps = pkg_def["runtime"].get("dependencies", [])
                if isinstance(rt_deps, list):
                    deps.extend(rt_deps)

            for dep in deps:
                if isinstance(dep, str):
                    visit(dep, path + [pkg_id])
                elif isinstance(dep, dict) and "id" in dep:
                    visit(dep["id"], path + [pkg_id])

            in_stack.remove(pkg_id)
            visited.add(pkg_id)
            install_order.append(pkg_id)

        visit(package_id, [])
        return install_order

    def check_removal_safety(self, package_id: str) -> List[str]:
        """
        Checks whether removing package_id would break any other installed package that depends on it.
        Returns a list of dependent installed package IDs.
        """
        installed = self.registry.list_installed()
        dependents = []

        for inst_id, inst_info in installed.items():
            if inst_id == package_id:
                continue
            pkg_def = self.catalogue.get_package_definition(inst_id)
            if not pkg_def:
                continue
            deps = []
            if "dependencies" in pkg_def and isinstance(pkg_def["dependencies"], list):
                deps.extend(pkg_def["dependencies"])
            if "runtime" in pkg_def and isinstance(pkg_def["runtime"], dict):
                rt_deps = pkg_def["runtime"].get("dependencies", [])
                if isinstance(rt_deps, list):
                    deps.extend(rt_deps)

            for dep in deps:
                dep_id = dep if isinstance(dep, str) else (dep.get("id") if isinstance(dep, dict) else None)
                if dep_id == package_id:
                    dependents.append(inst_id)
                    break

        return dependents
