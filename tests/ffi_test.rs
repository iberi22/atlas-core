use atlas::ffi::*;
use std::ffi::{CStr, CString};

#[test]
fn test_ffi_can_transition() {
    // 100 -> 101 is valid
    assert_eq!(atlas_status_can_transition(100, 101), 1);
    // 100 -> 130 is illegal
    assert_eq!(atlas_status_can_transition(100, 130), 0);
    // Invalid codes
    assert_eq!(atlas_status_can_transition(9999, 101), 0);
}

#[test]
fn test_ffi_slug_and_free() {
    let ptr = atlas_status_slug(103);
    assert!(!ptr.is_null());
    let slug = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
    assert_eq!(slug, "TSK_RUNNING");
    atlas_free_string(ptr);
}

#[test]
fn test_ffi_exec_cabi() {
    let input = CString::new("{\"task_id\":\"TSK-01\"}").unwrap();
    let res_ptr = atlas_exec_cabi(input.as_ptr());
    assert!(!res_ptr.is_null());
    let res_str = unsafe { CStr::from_ptr(res_ptr).to_str().unwrap() };
    assert!(res_str.contains("\"sodp_version\":\"4.0\""));
    atlas_free_string(res_ptr);
}
