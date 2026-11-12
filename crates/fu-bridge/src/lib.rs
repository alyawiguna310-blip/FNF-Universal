//! C ABI bridge for project operations.
//!
//! The caller owns all input and output memory. For each operation, call once
//! with a null output pointer and zero capacity to obtain the required byte
//! length, allocate that many bytes, then call again to write the JSON result.
//! If the supplied output buffer is too small, the function returns the
//! negative required length. A negative return is never a byte count.

use fu_project::{ProjectDocument, ProjectMetadata};
use serde_json::{json, Value};
use std::slice;
use std::str;

#[no_mangle]
pub extern "C" fn fu_core_version() -> u32 {
    2
}

#[no_mangle]
pub extern "C" fn fu_bridge_api_version() -> u32 {
    1
}

/// Create an empty project from a JSON-encoded ProjectMetadata value.
///
/// Returns a JSON ProjectDocument, or {"error":"..."} on failure.
#[no_mangle]
pub unsafe extern "C" fn fu_project_create_json(
    input_ptr: *const u8,
    input_len: usize,
    output_ptr: *mut u8,
    output_capacity: usize,
) -> isize {
    let result = with_input(input_ptr, input_len, |source| {
        let metadata: ProjectMetadata = serde_json::from_str(source)
            .map_err(|error| format!("invalid project metadata: {error}"))?;
        let project = ProjectDocument::new(metadata);
        project
            .to_pretty_json()
            .map_err(|error| format!("could not serialize project: {error}"))
    });
    write_result(result, output_ptr, output_capacity)
}

/// Validate a JSON-encoded ProjectDocument.
///
/// Returns {"valid":true,"issues":[]} or a structured validation response.
#[no_mangle]
pub unsafe extern "C" fn fu_project_validate_json(
    input_ptr: *const u8,
    input_len: usize,
    output_ptr: *mut u8,
    output_capacity: usize,
) -> isize {
    let result = with_input(input_ptr, input_len, |source| {
        let project = ProjectDocument::from_json(source)
            .map_err(|error| format!("invalid project JSON: {error}"))?;

        let response = match project.validate() {
            Ok(()) => json!({ "valid": true, "issues": [] }),
            Err(issues) => json!({ "valid": false, "issues": issues }),
        };
        serde_json::to_string(&response)
            .map_err(|error| format!("could not serialize validation result: {error}"))
    });
    write_result(result, output_ptr, output_capacity)
}

/// Parse, validate and return canonical pretty-printed project JSON.
///
/// Invalid projects return {"error":"...","issues":[...]}.
#[no_mangle]
pub unsafe extern "C" fn fu_project_normalize_json(
    input_ptr: *const u8,
    input_len: usize,
    output_ptr: *mut u8,
    output_capacity: usize,
) -> isize {
    let result = with_input(input_ptr, input_len, |source| {
        let project = ProjectDocument::from_json(source)
            .map_err(|error| format!("invalid project JSON: {error}"))?;
        if let Err(issues) = project.validate() {
            let response = json!({
                "error": "project validation failed",
                "issues": issues
            });
            return serde_json::to_string(&response)
                .map_err(|error| format!("could not serialize error response: {error}"));
        }
        project
            .to_pretty_json()
            .map_err(|error| format!("could not serialize project: {error}"))
    });
    write_result(result, output_ptr, output_capacity)
}

unsafe fn with_input(
    input_ptr: *const u8,
    input_len: usize,
    operation: impl FnOnce(&str) -> Result<String, String>,
) -> Result<String, String> {
    if input_ptr.is_null() {
        return Err("input pointer must not be null".into());
    }
    let bytes = slice::from_raw_parts(input_ptr, input_len);
    let source = str::from_utf8(bytes).map_err(|error| format!("input is not UTF-8: {error}"))?;
    operation(source)
}

fn write_result(
    result: Result<String, String>,
    output_ptr: *mut u8,
    output_capacity: usize,
) -> isize {
    let payload = match result {
        Ok(value) => value,
        Err(message) => json!({ "error": message }).to_string(),
    };
    write_bytes(payload.as_bytes(), output_ptr, output_capacity)
}

fn write_bytes(bytes: &[u8], output_ptr: *mut u8, output_capacity: usize) -> isize {
    let Ok(required) = isize::try_from(bytes.len()) else {
        return isize::MIN;
    };

    // Query mode: no output pointer and zero capacity.
    if output_ptr.is_null() && output_capacity == 0 {
        return required;
    }
    if output_ptr.is_null() {
        return -1;
    }
    if output_capacity < bytes.len() {
        return -required;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output_ptr, bytes.len());
    }
    required
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_buffered(
        function: unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> isize,
        input: &str,
    ) -> String {
        let required = unsafe {
            function(input.as_ptr(), input.len(), std::ptr::null_mut(), 0)
        };
        assert!(required > 0);
        let mut output = vec![0_u8; required as usize];
        let written = unsafe {
            function(
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
            )
        };
        assert_eq!(written, required);
        String::from_utf8(output).unwrap()
    }

    const METADATA: &str = r#"{"id":"demo","name":"Demo Mod","targetEngine":null}"#;
    const PROJECT: &str = r#"{
        "format":"fnf-universal-project",
        "formatVersion":1,
        "project":{"id":"demo","name":"Demo Mod","targetEngine":null},
        "songs":[],"characters":[],"stages":[],"assets":[],"modSettings":[]
    }"#;

    #[test]
    fn reports_bridge_api_version() {
        assert_eq!(fu_bridge_api_version(), 1);
    }

    #[test]
    fn creates_project_json() {
        let output = call_buffered(fu_project_create_json, METADATA);
        let project = ProjectDocument::from_json(&output).unwrap();
        assert_eq!(project.project.id, "demo");
        assert!(project.validate().is_ok());
    }

    #[test]
    fn validates_project_json() {
        let output = call_buffered(fu_project_validate_json, PROJECT);
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["valid"], true);
    }

    #[test]
    fn rejects_invalid_project_content() {
        let invalid = PROJECT.replace("\"name\":\"Demo Mod\"", "\"name\":\"\"");
        let output = call_buffered(fu_project_validate_json, &invalid);
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["valid"], false);
        assert!(!value["issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn normalize_returns_canonical_project() {
        let output = call_buffered(fu_project_normalize_json, PROJECT);
        let project = ProjectDocument::from_json(&output).unwrap();
        assert!(project.validate().is_ok());
    }

    #[test]
    fn too_small_output_reports_required_capacity() {
        let input = METADATA.as_bytes();
        let mut tiny = [0_u8; 1];
        let result = unsafe {
            fu_project_create_json(
                input.as_ptr(),
                input.len(),
                tiny.as_mut_ptr(),
                tiny.len(),
            )
        };
        assert!(result < -1);
    }

    #[test]
    fn invalid_json_returns_error_payload() {
        let output = call_buffered(fu_project_validate_json, "{");
        let value: Value = serde_json::from_str(&output).unwrap();
        assert!(value["error"].as_str().is_some());
    }
}
