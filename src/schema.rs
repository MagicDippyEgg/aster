//! Schema validation helpers for Aster JSON files.

use crate::error::{AsterError, Result};
use crate::util::py_str;
use serde_json::{Map, Value};

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

    let pkg_id = obj.get("id").and_then(|v| v.as_str());
    if pkg_id.is_none_or(|s| s.is_empty()) {
        return Err(AsterError::Validation(
            "Package 'id' must be a non-empty string.".to_string(),
        ));
    }

    Ok(())
}

pub fn validate_index(data: &Value) -> Result<()> {
    let obj = data
        .as_object()
        .ok_or_else(|| AsterError::Validation("Index must be a JSON object.".to_string()))?;
    match obj.get("packages") {
        Some(Value::Object(_)) => Ok(()),
        _ => Err(AsterError::Validation(
            "Index missing 'packages' dictionary.".to_string(),
        )),
    }
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
