"""
Schema validation helpers for Aster JSON files.
"""

class ValidationError(Exception):
    pass

def validate_package_definition(data: dict) -> None:
    """Validates a package definition dictionary."""
    if not isinstance(data, dict):
        raise ValidationError("Package definition must be a JSON object.")

    required_fields = ["schema_version", "id", "name", "version", "type"]
    for field in required_fields:
        if field not in data:
            raise ValidationError(f"Missing required field in package definition: '{field}'")

    pkg_type = data.get("type")
    if pkg_type not in ("source", "binary"):
        raise ValidationError(f"Invalid package type '{pkg_type}'. Expected 'source' or 'binary'.")

    pkg_id = data.get("id")
    if not isinstance(pkg_id, str) or not pkg_id:
        raise ValidationError("Package 'id' must be a non-empty string.")

def validate_index(data: dict) -> None:
    """Validates an index.json dictionary."""
    if not isinstance(data, dict):
        raise ValidationError("Index must be a JSON object.")
    if "packages" not in data or not isinstance(data["packages"], dict):
        raise ValidationError("Index missing 'packages' dictionary.")

def validate_registry(data: dict) -> None:
    """Validates a packages.json registry dictionary."""
    if not isinstance(data, dict):
        raise ValidationError("Registry must be a JSON object.")
    if "installed" not in data or not isinstance(data["installed"], dict):
        raise ValidationError("Registry missing 'installed' dictionary.")
