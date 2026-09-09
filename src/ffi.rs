use crate::protocol::SwalStatusCode;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Check if a SODP state transition is permitted (ADR-004).
/// Returns 1 if allowed, 0 if illegal or invalid code.
#[unsafe(no_mangle)]
pub extern "C" fn atlas_status_can_transition(from_code: i64, to_code: i64) -> i32 {
    match (SwalStatusCode::from_code(from_code), SwalStatusCode::from_code(to_code)) {
        (Some(from), Some(to)) => {
            if SwalStatusCode::can_transition(from, to) {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Returns the slug string for a status code.
/// Caller must free with `atlas_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn atlas_status_slug(code: i64) -> *mut c_char {
    match SwalStatusCode::from_code(code) {
        Some(status) => match CString::new(status.slug()) {
            Ok(c_str) => c_str.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// Free a string returned by Atlas C-ABI exports.
#[unsafe(no_mangle)]
pub extern "C" fn atlas_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

/// Execute JSON-based FFI dispatch (Archetype SNP-903).
#[unsafe(no_mangle)]
pub extern "C" fn atlas_exec_cabi(json_input: *const c_char) -> *mut c_char {
    if json_input.is_null() {
        return std::ptr::null_mut();
    }
    let input_str = unsafe { CStr::from_ptr(json_input).to_string_lossy() };
    let response = format!(
        "{{\"status\":\"ok\",\"sodp_version\":\"4.0\",\"input_len\":{}}}",
        input_str.len()
    );
    match CString::new(response) {
        Ok(c_str) => c_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
