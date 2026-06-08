#include <stdarg.h>
#include <stdint.h>
#include <stdlib.h>

/// Opaque handle to a CassetteDB database instance.
///
/// This type is intentionally opaque; C code should treat it as an incomplete
/// struct and only manipulate it through the provided API functions.
typedef struct CassetteDB CassetteDB;

#ifdef __cplusplus
extern "C" {
#endif

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
CassetteDB *cassette_db_open(const char *path);

/// Close a database handle and free associated resources.
///
/// # Arguments
/// * `db` - Pointer returned by `cassette_db_open`.
///
/// # Safety
/// * `db` must be a valid, non-null pointer returned by `cassette_db_open`.
/// * After this call, `db` is invalid and must not be used again.
void cassette_db_close(CassetteDB *db);

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
char *cassette_insert(CassetteDB *db, const char *json);

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
char *cassette_get(CassetteDB *db, const char *id);

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
int cassette_update(CassetteDB *db, const char *id, const char *json);

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
int cassette_delete(CassetteDB *db, const char *id);

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
char *cassette_query(CassetteDB *db, const char *query);

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
char *cassette_dump(CassetteDB *db);

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
int cassette_compact(CassetteDB *db);

/// Return the last error message from the current thread.
///
/// # Returns
/// * A newly allocated string containing the last error message, or an empty
///   string if no error has occurred.
/// * `NULL` only if allocation fails.
///
/// # Safety
/// * The returned string must be freed with `cassette_free_string`.
char *cassette_last_error();

/// Free a string previously returned by this library.
///
/// # Arguments
/// * `s` - Pointer to a string allocated by the Rust heap.
///
/// # Safety
/// * `s` must be a pointer previously returned by this library (or `NULL`).
/// * After this call, `s` is invalid and must not be used again.
void cassette_free_string(char *s);

#ifdef __cplusplus
}  // extern "C"
#endif
