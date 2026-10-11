//! Package dependency resolution and cycle detection.

use crate::catalogue::CatalogueManager;
use crate::error::{AsterError, Result};
use crate::registry::RegistryManager;
use serde_json::Value;

#[derive(Clone)]
pub struct DependencyResolver {
    catalogue: CatalogueManager,
    registry: RegistryManager,
}

impl DependencyResolver {
    pub fn new(catalogue: CatalogueManager, registry: RegistryManager) -> Self {
        DependencyResolver {
            catalogue,
            registry,
        }
    }

    /// Resolves package dependencies recursively.
    ///
    /// Returns an ordered list of package IDs to install (dependencies first,
    /// `package_id` last). Raises a `Dependency` error on circular dependencies.
    pub fn resolve_dependencies(&self, package_id: &str) -> Result<Vec<String>> {
        let mut install_order: Vec<String> = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut in_stack: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut path: Vec<String> = Vec::new();

        self.visit(
            package_id,
            &mut path,
            &mut visited,
            &mut in_stack,
            &mut install_order,
        )?;

        Ok(install_order)
    }

    fn visit(
        &self,
        pkg_id: &str,
        path: &mut Vec<String>,
        visited: &mut std::collections::HashSet<String>,
        in_stack: &mut std::collections::HashSet<String>,
        install_order: &mut Vec<String>,
    ) -> Result<()> {
        if in_stack.contains(pkg_id) {
            let mut cycle_path = path.clone();
            cycle_path.push(pkg_id.to_string());
            return Err(AsterError::Dependency(format!(
                "Circular dependency detected: {}",
                cycle_path.join(" -> ")
            )));
        }
        if visited.contains(pkg_id) {
            return Ok(());
        }

        in_stack.insert(pkg_id.to_string());
        path.push(pkg_id.to_string());

        let pkg_def = self.catalogue.get_package_definition(pkg_id, None)?;
        let pkg_def = match pkg_def {
            Some(d) => d,
            None => {
                path.pop();
                in_stack.remove(pkg_id);
                return Err(AsterError::Dependency(format!(
                    "Dependency package definition '{pkg_id}' not found in catalogue."
                )));
            }
        };

        for dep in collect_dependencies(&pkg_def, true) {
            self.visit(&dep, path, visited, in_stack, install_order)?;
        }

        path.pop();
        in_stack.remove(pkg_id);
        visited.insert(pkg_id.to_string());
        install_order.push(pkg_id.to_string());
        Ok(())
    }

    /// Checks whether removing `package_id` would break any installed package.
    ///
    /// Returns the IDs of dependent installed packages.
    pub fn check_removal_safety(&self, package_id: &str) -> Result<Vec<String>> {
        let installed = self.registry.list_installed()?;
        let mut dependents: Vec<String> = Vec::new();

        for (inst_id, _) in installed.iter() {
            if inst_id == package_id {
                continue;
            }
            let pkg_def = match self.catalogue.get_package_definition(inst_id, None)? {
                Some(d) => d,
                None => continue,
            };
            for dep in collect_dependencies(&pkg_def, false) {
                if dep == package_id {
                    dependents.push(inst_id.clone());
                    break;
                }
            }
        }

        Ok(dependents)
    }
}

/// Extracts dependency IDs. When `include_build` is false only `dependencies`
/// and `runtime.dependencies` are considered.
fn collect_dependencies(pkg_def: &Value, include_build: bool) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();

    if let Some(arr) = pkg_def.get("dependencies").and_then(|v| v.as_array()) {
        deps.extend(arr.iter().filter_map(dep_id));
    }

    if include_build {
        match pkg_def.get("build") {
            Some(build) if build.is_object() => {
                if let Some(arr) = build.get("dependencies").and_then(|v| v.as_array()) {
                    deps.extend(arr.iter().filter_map(dep_id));
                }
            }
            _ => {
                if let Some(arr) = pkg_def.get("build_dependencies").and_then(|v| v.as_array()) {
                    deps.extend(arr.iter().filter_map(dep_id));
                }
            }
        }
    }

    match pkg_def.get("runtime") {
        Some(runtime) if runtime.is_object() => {
            if let Some(arr) = runtime.get("dependencies").and_then(|v| v.as_array()) {
                deps.extend(arr.iter().filter_map(dep_id));
            }
        }
        _ => {
            if let Some(arr) = pkg_def
                .get("runtime_dependencies")
                .and_then(|v| v.as_array())
            {
                deps.extend(arr.iter().filter_map(dep_id));
            }
        }
    }

    deps
}

fn dep_id(dep: &Value) -> Option<String> {
    match dep {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}
