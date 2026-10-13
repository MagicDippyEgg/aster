//! Schema validation helpers for Aster JSON files.

use crate::error::{AsterError, Result};
use crate::util::py_str;
use serde_json::{Map, Value};

pub fn validate_package_id(package_id: &str) -> Result<()> {
    if package_id.is_empty()
        || package_id == "."
        || package_id == ".."
        || package_id.contains('/')
        || package_id.contains('\\')
        || package_id.contains('\0')
    {
        return Err(AsterError::Validation(format!(
            "Invalid package ID '{package_id}': IDs must be non-empty and cannot contain path separators or NUL."
        )));
    }
    Ok(())
}

pub fn validate_binary_name(binary_name: &str) -> Result<()> {
    if binary_name.is_empty()
        || binary_name == "."
        || binary_name == ".."
        || binary_name.contains('/')
        || binary_name.contains('\\')
        || binary_name.contains('\0')
    {
        return Err(AsterError::Validation(format!(
            "Invalid command name '{binary_name}': names cannot contain path separators or NUL."
        )));
    }
    Ok(())
}

pub fn validate_repository_name(repository_name: &str) -> Result<()> {
    if repository_name.is_empty()
        || repository_name == "."
        || repository_name == ".."
        || repository_name.contains('/')
        || repository_name.contains('\\')
        || repository_name.contains('\0')
    {
        return Err(AsterError::Validation(format!(
            "Invalid repository name '{repository_name}': names cannot contain path separators or NUL."
        )));
    }
    Ok(())
}

pub fn validate_package_definition(data: &Value) -> Result<()> {
    let obj = data.as_object().ok_or_else(|| {
        AsterError::Validation("Package definition must be a JSON object.".to_string())
    })?;

    for field in ["schema_version", "id", "name", "version", "type"] {
        if !obj.contains_key(field) {
            return Err(AsterError::Validation(format!(
                "Missing required field in package definition: '{field}'"
            )));
        }
    }

    let pkg_type = obj.get("type").and_then(|v| v.as_str());
    match pkg_type {
        Some("source") | Some("binary") => {}
        _ => {
            let rendered = match obj.get("type") {
                Some(v) => py_str(v),
                None => "None".to_string(),
            };
            return Err(AsterError::Validation(format!(
                "Invalid package type '{rendered}'. Expected 'source' or 'binary'."
            )));
        }
    }

    let pkg_id = obj.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
        AsterError::Validation("Package 'id' must be a non-empty string.".to_string())
    })?;
    validate_package_id(pkg_id)?;

    Ok(())
}

pub fn validate_index(data: &Value) -> Result<()> {
    let obj = data
        .as_object()
        .ok_or_else(|| AsterError::Validation("Index must be a JSON object.".to_string()))?;
    match obj.get("packages") {
        Some(Value::Object(_)) => {}
        _ => {
            return Err(AsterError::Validation(
                "Index missing 'packages' dictionary.".to_string(),
            ))
        }
    }

    if let Some(sv) = obj.get("schema_version") {
        let version = sv
            .as_i64()
            .or_else(|| sv.as_u64().map(|v| v as i64));
        match version {
            Some(1) | Some(2) => {}
            Some(v) => {
                return Err(AsterError::Validation(format!(
                    "Unsupported index schema_version '{v}'. Supported versions are 1 and 2."
                )))
            }
            None => {
                return Err(AsterError::Validation(
                    "Field 'schema_version' in index must be an integer.".to_string(),
                ))
            }
        }
    }

    Ok(())
}

pub fn validate_registry(data: &Value) -> Result<()> {
    let obj = data
        .as_object()
        .ok_or_else(|| AsterError::Validation("Registry must be a JSON object.".to_string()))?;
    match obj.get("installed") {
        Some(Value::Object(_)) => Ok(()),
        _ => Err(AsterError::Validation(
            "Registry missing 'installed' dictionary.".to_string(),
        )),
    }
}

// Keep Map in scope for downstream convenience.
pub type JsonObject = Map<String, Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_ids_reject_path_traversal_and_nul() {
        for id in [
            "",
            ".",
            "..",
            "../escape",
            "pkg/escape",
            "pkg\\escape",
            "nul\0id",
        ] {
            assert!(validate_package_id(id).is_err(), "should reject {id:?}");
        }
    }

    #[test]
    fn package_ids_allow_common_package_name_characters() {
        for id in ["fastfetch-bin", "python3.12", "lib_foo", "c++", "7zip"] {
            assert!(validate_package_id(id).is_ok(), "should accept {id:?}");
        }
    }

    #[test]
    fn validate_index_schema_versions() {
        use serde_json::json;

        // Schema version 1
        assert!(validate_index(&json!({"schema_version": 1, "packages": {}})).is_ok());

        // Schema version 2
        assert!(validate_index(&json!({"schema_version": 2, "packages": {}})).is_ok());

        // Schema version 2 with metadata
        assert!(validate_index(&json!({
            "schema_version": 2,
            "description": "Test Repo",
            "packages": {}
        })).is_ok());

        // Missing schema_version (backwards compatible defaults to 1)
        assert!(validate_index(&json!({"packages": {}})).is_ok());

        // Unsupported schema_version
        assert!(validate_index(&json!({"schema_version": 3, "packages": {}})).is_err());

        // Non-integer schema_version
        assert!(validate_index(&json!({"schema_version": "2", "packages": {}})).is_err());

        // Missing packages
        assert!(validate_index(&json!({"schema_version": 2})).is_err());
    }

    #[test]
    fn command_and_repository_names_reject_path_components() {
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "pkg/escape",
            "pkg\\escape",
            "nul\0name",
        ] {
            assert!(
                validate_binary_name(name).is_err(),
                "should reject command {name:?}"
            );
            assert!(
                validate_repository_name(name).is_err(),
                "should reject repository {name:?}"
            );
        }
        assert!(validate_binary_name("fastfetch").is_ok());
        assert!(validate_repository_name("community-x86_64").is_ok());
    }
}
