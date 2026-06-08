//! C FFI bindings for CassetteDB.
//!
//! This crate exposes a C-compatible API for the Rust CassetteDB engine.
//! All functions prefixed with `cassette_` are intended for consumption from C.
//!
//! # Memory Safety
//! - All `*mut c_char` strings returned by this API are allocated on the Rust heap
//!   and must be freed by the caller using `cassette_free_string`.
//! - The database handle (`*mut CassetteDB`) is an opaque pointer.
//!   Create it with `cassette_db_open` and destroy it with `cassette_db_close`.
//! - After calling `cassette_db_close`, the handle is invalid and must not be used.
//!
//! # Error Handling
//! - Functions that return `c_int` return `0` on success and `-1` on error.
//! - On error, call `cassette_last_error` to retrieve a human-readable message.
//! - The error message string must also be freed with `cassette_free_string`.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int};
use std::path::Path;

use cassettedb::engine::CassetteEngine;
use cassettedb::document::Document;
use cassettedb::query::Query;

/// Opaque handle to a CassetteDB database instance.
///
/// This type is intentionally opaque; C code should treat it as an incomplete
/// struct and only manipulate it through the provided API functions.
pub struct CassetteDB {
    engine: CassetteEngine,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = RefCell::new(None);
}

/// Store an error message in thread-local storage.
fn set_error(msg: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = Some(msg);
    });
}

/// Clear the last error.
fn clear_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

/// Convert a Rust `Result<T>` into a C status code, storing any error.
fn handle_result<T>(res: Result<T, cassettedb::error::CassetteError>) -> c_int {
    match res {
        Ok(_) => {
            clear_error();
            0
        }
        Err(e) => {
            set_error(e.to_string());
            -1
        }
    }
}

/// Open (or create) a CassetteDB database at the given path.
///
/// # Arguments
/// * `path` - Null-terminated UTF-8 file path string.
///
/// # Returns
/// * A pointer to an opaque `CassetteDB` handle on success.
/// * `NULL` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `path` must be a valid, null-terminated C string.
/// * The returned pointer must be freed with `cassette_db_close`.
#[no_mangle]
pub unsafe extern "C" fn cassette_db_open(path: *const c_char) -> *mut CassetteDB {
    clear_error();
    if path.is_null() {
        set_error("Null path pointer".to_string());
        return std::ptr::null_mut();
    }
    let c_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("Path is not valid UTF-8".to_string());
            return std::ptr::null_mut();
        }
    };
    match CassetteEngine::open(Path::new(c_str)) {
        Ok(engine) => {
            let db = Box::new(CassetteDB { engine });
            Box::into_raw(db)
        }
        Err(e) => {
            set_error(e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Close a database handle and free associated resources.
///
/// # Arguments
/// * `db` - Pointer returned by `cassette_db_open`.
///
/// # Safety
/// * `db` must be a valid, non-null pointer returned by `cassette_db_open`.
/// * After this call, `db` is invalid and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn cassette_db_close(db: *mut CassetteDB) {
    if !db.is_null() {
        drop(Box::from_raw(db));
    }
}

/// Insert a new JSON document into the database.
///
/// # Arguments
/// * `db` - Valid database handle.
/// * `json` - Null-terminated UTF-8 JSON string.
///
/// # Returns
/// * A newly allocated string containing the document ID on success.
/// * `NULL` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db` must be a valid, non-null pointer.
/// * `json` must be a valid, null-terminated C string.
/// * The returned string must be freed with `cassette_free_string`.
#[no_mangle]
pub unsafe extern "C" fn cassette_insert(db: *mut CassetteDB, json: *const c_char) -> *mut c_char {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return std::ptr::null_mut();
    }
    if json.is_null() {
        set_error("Null JSON pointer".to_string());
        return std::ptr::null_mut();
    }
    let json_str = match CStr::from_ptr(json).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("JSON is not valid UTF-8".to_string());
            return std::ptr::null_mut();
        }
    };
    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            set_error(format!("Invalid JSON: {}", e));
            return std::ptr::null_mut();
        }
    };
    let doc = Document::new(value);
    match (*db).engine.insert(doc) {
        Ok(id) => match CString::new(id) {
            Ok(c_id) => c_id.into_raw(),
            Err(_) => {
                set_error("Document ID contains null byte".to_string());
                std::ptr::null_mut()
            }
        },
        Err(e) => {
            set_error(e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Retrieve a document by its ID.
///
/// # Arguments
/// * `db` - Valid database handle.
/// * `id` - Null-terminated document ID string.
///
/// # Returns
/// * A newly allocated JSON string representing the document on success.
/// * `NULL` if the document is not found or on error.
///   Call `cassette_last_error` to distinguish (error will be empty for "not found").
///
/// # Safety
/// * `db` and `id` must be valid, non-null pointers.
/// * The returned string must be freed with `cassette_free_string`.
#[no_mangle]
pub unsafe extern "C" fn cassette_get(db: *mut CassetteDB, id: *const c_char) -> *mut c_char {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return std::ptr::null_mut();
    }
    if id.is_null() {
        set_error("Null ID pointer".to_string());
        return std::ptr::null_mut();
    }
    let id_str = match CStr::from_ptr(id).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("ID is not valid UTF-8".to_string());
            return std::ptr::null_mut();
        }
    };
    match (*db).engine.get(id_str) {
        Some(doc) => match serde_json::to_string(doc) {
            Ok(json) => match CString::new(json) {
                Ok(c_json) => c_json.into_raw(),
                Err(_) => {
                    set_error("Document JSON contains null byte".to_string());
                    std::ptr::null_mut()
                }
            },
            Err(e) => {
                set_error(format!("Serialization error: {}", e));
                std::ptr::null_mut()
            }
        },
        None => {
            set_error(format!("Document not found: {}", id_str));
            std::ptr::null_mut()
        }
    }
}

/// Update an existing document by ID.
///
/// # Arguments
/// * `db` - Valid database handle.
/// * `id` - Null-terminated document ID string.
/// * `json` - Null-terminated UTF-8 JSON string with the new document data.
///
/// # Returns
/// * `0` on success.
/// * `-1` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db`, `id`, and `json` must be valid, non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn cassette_update(
    db: *mut CassetteDB,
    id: *const c_char,
    json: *const c_char,
) -> c_int {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return -1;
    }
    if id.is_null() {
        set_error("Null ID pointer".to_string());
        return -1;
    }
    if json.is_null() {
        set_error("Null JSON pointer".to_string());
        return -1;
    }
    let id_str = match CStr::from_ptr(id).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("ID is not valid UTF-8".to_string());
            return -1;
        }
    };
    let json_str = match CStr::from_ptr(json).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("JSON is not valid UTF-8".to_string());
            return -1;
        }
    };
    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            set_error(format!("Invalid JSON: {}", e));
            return -1;
        }
    };
    handle_result((*db).engine.update(id_str, value))
}

/// Delete a document by ID.
///
/// # Arguments
/// * `db` - Valid database handle.
/// * `id` - Null-terminated document ID string.
///
/// # Returns
/// * `0` on success.
/// * `-1` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db` and `id` must be valid, non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn cassette_delete(db: *mut CassetteDB, id: *const c_char) -> c_int {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return -1;
    }
    if id.is_null() {
        set_error("Null ID pointer".to_string());
        return -1;
    }
    let id_str = match CStr::from_ptr(id).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("ID is not valid UTF-8".to_string());
            return -1;
        }
    };
    handle_result((*db).engine.delete(id_str))
}

/// Execute a query against the database.
///
/// # Arguments
/// * `db` - Valid database handle.
/// * `query` - Null-terminated query string (e.g. `age > 28`, `search("hello")`, `*`).
///
/// # Returns
/// * A newly allocated JSON array string containing matched documents on success.
/// * `NULL` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db` and `query` must be valid, non-null pointers.
/// * The returned string must be freed with `cassette_free_string`.
#[no_mangle]
pub unsafe extern "C" fn cassette_query(
    db: *mut CassetteDB,
    query: *const c_char,
) -> *mut c_char {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return std::ptr::null_mut();
    }
    if query.is_null() {
        set_error("Null query pointer".to_string());
        return std::ptr::null_mut();
    }
    let query_str = match CStr::from_ptr(query).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error("Query is not valid UTF-8".to_string());
            return std::ptr::null_mut();
        }
    };
    let q = match Query::parse(query_str) {
        Ok(q) => q,
        Err(e) => {
            set_error(format!("Query parse error: {}", e));
            return std::ptr::null_mut();
        }
    };
    let result = (*db).engine.query(&q);
    match serde_json::to_string(&result.documents) {
        Ok(json) => match CString::new(json) {
            Ok(c_json) => c_json.into_raw(),
            Err(_) => {
                set_error("Result JSON contains null byte".to_string());
                std::ptr::null_mut()
            }
        },
        Err(e) => {
            set_error(format!("Serialization error: {}", e));
            std::ptr::null_mut()
        }
    }
}

/// Dump all documents in the database as a pretty-printed JSON array.
///
/// # Arguments
/// * `db` - Valid database handle.
///
/// # Returns
/// * A newly allocated JSON string on success.
/// * `NULL` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db` must be a valid, non-null pointer.
/// * The returned string must be freed with `cassette_free_string`.
#[no_mangle]
pub unsafe extern "C" fn cassette_dump(db: *mut CassetteDB) -> *mut c_char {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return std::ptr::null_mut();
    }
    match (*db).engine.dump() {
        Ok(json) => match CString::new(json) {
            Ok(c_json) => c_json.into_raw(),
            Err(_) => {
                set_error("Dump JSON contains null byte".to_string());
                std::ptr::null_mut()
            }
        },
        Err(e) => {
            set_error(e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Compact the database: rewrite the main file and truncate the WAL.
///
/// # Arguments
/// * `db` - Valid database handle.
///
/// # Returns
/// * `0` on success.
/// * `-1` on error. Call `cassette_last_error` for details.
///
/// # Safety
/// * `db` must be a valid, non-null pointer.
#[no_mangle]
pub unsafe extern "C" fn cassette_compact(db: *mut CassetteDB) -> c_int {
    clear_error();
    if db.is_null() {
        set_error("Null database pointer".to_string());
        return -1;
    }
    handle_result((*db).engine.compact())
}

/// Return the last error message from the current thread.
///
/// # Returns
/// * A newly allocated string containing the last error message, or an empty
///   string if no error has occurred.
/// * `NULL` only if allocation fails.
///
/// # Safety
/// * The returned string must be freed with `cassette_free_string`.
#[no_mangle]
pub extern "C" fn cassette_last_error() -> *mut c_char {
    let msg = LAST_ERROR.with(|e| e.borrow().clone().unwrap_or_default());
    match CString::new(msg) {
        Ok(c_msg) => c_msg.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string previously returned by this library.
///
/// # Arguments
/// * `s` - Pointer to a string allocated by the Rust heap.
///
/// # Safety
/// * `s` must be a pointer previously returned by this library (or `NULL`).
/// * After this call, `s` is invalid and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn cassette_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use tempfile::TempDir;

    #[test]
    fn test_open_close() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());
            cassette_db_close(db);
        }
    }

    #[test]
    fn test_insert_get_free() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());

            let json = CString::new(r#"{"name":"Alice","age":30}"#).unwrap();
            let id_ptr = cassette_insert(db, json.as_ptr());
            assert!(!id_ptr.is_null());

            let id = CStr::from_ptr(id_ptr).to_str().unwrap();
            assert!(!id.is_empty());

            let get_ptr = cassette_get(db, id_ptr);
            assert!(!get_ptr.is_null());
            let got = CStr::from_ptr(get_ptr).to_str().unwrap();
            assert!(got.contains("Alice"));

            cassette_free_string(id_ptr);
            cassette_free_string(get_ptr);
            cassette_db_close(db);
        }
    }

    #[test]
    fn test_update_delete() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());

            let json = CString::new(r#"{"title":"Old"}"#).unwrap();
            let id_ptr = cassette_insert(db, json.as_ptr());
            assert!(!id_ptr.is_null());

            let new_json = CString::new(r#"{"title":"New"}"#).unwrap();
            let rc = cassette_update(db, id_ptr, new_json.as_ptr());
            assert_eq!(rc, 0);

            let get_ptr = cassette_get(db, id_ptr);
            assert!(!get_ptr.is_null());
            let got = CStr::from_ptr(get_ptr).to_str().unwrap();
            assert!(got.contains("New"));
            cassette_free_string(get_ptr);

            let rc = cassette_delete(db, id_ptr);
            assert_eq!(rc, 0);

            let get_ptr2 = cassette_get(db, id_ptr);
            assert!(get_ptr2.is_null());
            cassette_free_string(id_ptr);
            cassette_db_close(db);
        }
    }

    #[test]
    fn test_query() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());

            let j1 = CString::new(r#"{"name":"Alice","age":30}"#).unwrap();
            let j2 = CString::new(r#"{"name":"Bob","age":25}"#).unwrap();
            let id1 = cassette_insert(db, j1.as_ptr());
            let id2 = cassette_insert(db, j2.as_ptr());
            assert!(!id1.is_null());
            assert!(!id2.is_null());

            let q = CString::new("age > 28").unwrap();
            let res_ptr = cassette_query(db, q.as_ptr());
            assert!(!res_ptr.is_null());
            let res = CStr::from_ptr(res_ptr).to_str().unwrap();
            assert!(res.contains("Alice"));
            assert!(!res.contains("Bob"));

            cassette_free_string(res_ptr);
            cassette_free_string(id1);
            cassette_free_string(id2);
            cassette_db_close(db);
        }
    }

    #[test]
    fn test_dump_and_compact() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());

            let j1 = CString::new(r#"{"x":1}"#).unwrap();
            let id1 = cassette_insert(db, j1.as_ptr());
            assert!(!id1.is_null());

            let dump_ptr = cassette_dump(db);
            assert!(!dump_ptr.is_null());
            let dump = CStr::from_ptr(dump_ptr).to_str().unwrap();
            assert!(dump.contains("x"));
            cassette_free_string(dump_ptr);

            let rc = cassette_compact(db);
            assert_eq!(rc, 0);

            cassette_free_string(id1);
            cassette_db_close(db);
        }
    }

    #[test]
    fn test_error_handling() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.cassette");
        let c_path = CString::new(db_path.to_str().unwrap()).unwrap();

        unsafe {
            let db = cassette_db_open(c_path.as_ptr());
            assert!(!db.is_null());

            let bad_json = CString::new("not json").unwrap();
            let id_ptr = cassette_insert(db, bad_json.as_ptr());
            assert!(id_ptr.is_null());

            let err_ptr = cassette_last_error();
            assert!(!err_ptr.is_null());
            let err = CStr::from_ptr(err_ptr).to_str().unwrap();
            assert!(!err.is_empty());
            cassette_free_string(err_ptr);

            cassette_db_close(db);
        }
    }
}
