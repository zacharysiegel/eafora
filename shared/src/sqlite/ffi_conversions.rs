//! Convert raw `sqlite-wasm-rs` FFI outputs into owned Rust values. wasm32-only; native uses rusqlite.

use std::ffi::CStr;

use crate::error::AppError;

/// A text column as an owned `String`; `Err` if the column is NULL or not valid UTF-8.
pub(crate) fn column_text(statement: *mut sqlite_wasm_rs::sqlite3_stmt, index: std::os::raw::c_int) -> Result<String, AppError> {
    let text: *const std::os::raw::c_uchar = unsafe { sqlite_wasm_rs::sqlite3_column_text(statement, index) };
    if text.is_null() {
        return Err(AppError::from(format!("ffi_conversions: null text column {index}")));
    }

    let text: &CStr = unsafe { CStr::from_ptr(text as *const std::os::raw::c_char) };

    text.to_str()
        .map(|value| value.to_string())
        .map_err(|err| AppError::from(format!("ffi_conversions: non-utf8 text column {index}: {err}")))
}

/// The connection's last error message, or `"unknown sqlite error"` if none is set.
pub(crate) fn error_message(db: *mut sqlite_wasm_rs::sqlite3) -> String {
    let raw: *const std::os::raw::c_char = unsafe { sqlite_wasm_rs::sqlite3_errmsg(db) };
    if raw.is_null() {
        return "unknown sqlite error".to_string();
    }

    unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned()
}
