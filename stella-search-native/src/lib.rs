//! Native library for system search integration
//!
//! Provides C ABI exports for P/Invoke from .NET/Uno Platform.
//! Uses Windows Search (OLE DB) on Windows, Tracker3 (D-Bus) on Linux.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

#[cfg(windows)]
mod windows_search;

#[cfg(unix)]
mod linux_search;

/// Check if system search is available.
/// Returns 1 if available, 0 if not.
#[unsafe(no_mangle)]
pub extern "C" fn stella_is_available() -> i32 {
    #[cfg(windows)]
    {
        windows_search::is_available() as i32
    }

    #[cfg(unix)]
    {
        linux_search::is_available() as i32
    }
}

/// Search for files matching the query.
/// Returns a JSON string that must be freed with stella_free.
/// Returns null on error.
///
/// # Safety
/// - `query` must be a valid null-terminated UTF-8 string
/// - `extension` can be null, otherwise must be a valid null-terminated UTF-8 string
/// - Caller must free the returned pointer with stella_free
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stella_search(
    query: *const c_char,
    max_results: u32,
    extension: *const c_char,
) -> *mut c_char {
    if query.is_null() {
        return ptr::null_mut();
    }

    let query_str = match unsafe { CStr::from_ptr(query) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    let ext = if extension.is_null() {
        None
    } else {
        match unsafe { CStr::from_ptr(extension) }.to_str() {
            Ok(s) => Some(s.to_string()),
            Err(_) => None,
        }
    };

    #[cfg(windows)]
    let result = windows_search::search(query_str, max_results, ext.as_deref());

    #[cfg(unix)]
    let result = linux_search::search(query_str, max_results, ext.as_deref());

    match result {
        Ok(json) => match CString::new(json) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

/// Get the name of the active search backend.
/// Returns a static string, do NOT free.
#[unsafe(no_mangle)]
pub extern "C" fn stella_backend_name() -> *const c_char {
    #[cfg(windows)]
    {
        static NAME: &[u8] = b"WindowsSearch\0";
        NAME.as_ptr() as *const c_char
    }

    #[cfg(unix)]
    {
        static NAME: &[u8] = b"Tracker\0";
        NAME.as_ptr() as *const c_char
    }
}

/// Free memory allocated by stella_search.
///
/// # Safety
/// - `ptr` must have been returned by stella_search
/// - `ptr` must not have been freed before
/// - `ptr` can be null (no-op)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stella_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Get the last error message.
/// Returns a static string, do NOT free.
/// Returns null if no error occurred.
#[unsafe(no_mangle)]
pub extern "C" fn stella_get_error() -> *const c_char {
    // Thread-local error storage
    thread_local! {
        static LAST_ERROR: std::cell::RefCell<Option<CString>> = const { std::cell::RefCell::new(None) };
    }

    LAST_ERROR.with(|e| {
        e.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null())
    })
}
