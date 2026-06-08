// Package cassettedb provides Go bindings for CassetteDB via cgo.
//
// It wraps the C FFI exported by libcassettedb, offering a small,
// idiomatic Go API on top of the underlying single-file JSON document
// database.
//
// Example:
//
//	db, err := cassettedb.Open("mydb.cassette")
//	if err != nil {
//	    log.Fatal(err)
//	}
//	defer db.Close()
//
//	id, err := db.Insert(`{"name":"Ada","age":36}`)
//	if err != nil {
//	    log.Fatal(err)
//	}
//
//	doc, err := db.Get(id)
//	fmt.Println(doc)
package cassettedb

/*
#cgo LDFLAGS: -lcassettedb -lpthread -ldl -lm
#include <stdlib.h>
#include "../../../cassette.h"
*/
import "C"

import (
	"encoding/json"
	"fmt"
	"unsafe"
)

// DB is a handle to an open CassetteDB database.
type DB struct {
	ptr *C.CassetteDB
}

// CassetteDBError is returned when the underlying library reports a failure.
type CassetteDBError struct {
	Message string
}

func (e *CassetteDBError) Error() string {
	return e.Message
}

// lastError retrieves and frees the last error message from the current thread.
func lastError() string {
	cMsg := C.cassette_last_error()
	if cMsg == nil {
		return "unknown error"
	}
	defer C.cassette_free_string(cMsg)
	return C.GoString(cMsg)
}

// Open opens or creates a CassetteDB database at path.
func Open(path string) (*DB, error) {
	cPath := C.CString(path)
	defer C.free(unsafe.Pointer(cPath))

	db := C.cassette_db_open(cPath)
	if db == nil {
		return nil, &CassetteDBError{Message: lastError()}
	}
	return &DB{ptr: db}, nil
}

// Close releases the database handle and associated resources.
func (db *DB) Close() {
	if db.ptr != nil {
		C.cassette_db_close(db.ptr)
		db.ptr = nil
	}
}

// Insert stores a JSON document and returns the assigned document ID.
func (db *DB) Insert(json string) (string, error) {
	cJSON := C.CString(json)
	defer C.free(unsafe.Pointer(cJSON))

	cID := C.cassette_insert(db.ptr, cJSON)
	if cID == nil {
		return "", &CassetteDBError{Message: lastError()}
	}
	defer C.cassette_free_string(cID)
	return C.GoString(cID), nil
}

// Get retrieves a document by ID. The returned value is the raw JSON string.
func (db *DB) Get(id string) (string, error) {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))

	cDoc := C.cassette_get(db.ptr, cID)
	if cDoc == nil {
		return "", &CassetteDBError{Message: lastError()}
	}
	defer C.cassette_free_string(cDoc)
	return C.GoString(cDoc), nil
}

// GetJSON retrieves a document by ID and unmarshals it into v.
func (db *DB) GetJSON(id string, v any) error {
	raw, err := db.Get(id)
	if err != nil {
		return err
	}
	return json.Unmarshal([]byte(raw), v)
}

// Update replaces the document identified by id with the provided JSON.
func (db *DB) Update(id string, json string) error {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))
	cJSON := C.CString(json)
	defer C.free(unsafe.Pointer(cJSON))

	if C.cassette_update(db.ptr, cID, cJSON) != 0 {
		return &CassetteDBError{Message: lastError()}
	}
	return nil
}

// Delete removes the document identified by id.
func (db *DB) Delete(id string) error {
	cID := C.CString(id)
	defer C.free(unsafe.Pointer(cID))

	if C.cassette_delete(db.ptr, cID) != 0 {
		return &CassetteDBError{Message: lastError()}
	}
	return nil
}

// Query executes a CassetteDB query and returns the raw JSON array result.
func (db *DB) Query(query string) (string, error) {
	cQuery := C.CString(query)
	defer C.free(unsafe.Pointer(cQuery))

	cRes := C.cassette_query(db.ptr, cQuery)
	if cRes == nil {
		return "", &CassetteDBError{Message: lastError()}
	}
	defer C.cassette_free_string(cRes)
	return C.GoString(cRes), nil
}

// QueryJSON executes a query and unmarshals the result into v.
func (db *DB) QueryJSON(query string, v any) error {
	raw, err := db.Query(query)
	if err != nil {
		return err
	}
	return json.Unmarshal([]byte(raw), v)
}

// Dump returns all documents in the database as a JSON array string.
func (db *DB) Dump() (string, error) {
	cDump := C.cassette_dump(db.ptr)
	if cDump == nil {
		return "", &CassetteDBError{Message: lastError()}
	}
	defer C.cassette_free_string(cDump)
	return C.GoString(cDump), nil
}

// Compact rewrites the main database file and truncates the WAL.
func (db *DB) Compact() error {
	if C.cassette_compact(db.ptr) != 0 {
		return &CassetteDBError{Message: lastError()}
	}
	return nil
}

// String returns a short description of the database handle.
func (db *DB) String() string {
	return fmt.Sprintf("CassetteDB(%p)", db.ptr)
}
