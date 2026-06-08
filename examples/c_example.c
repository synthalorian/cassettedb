/*
 * CassetteDB C Example
 * 
 * Build with:
 *   gcc -o c_example c_example.c -L../target/release -lcassettedb -Wl,-rpath,../target/release
 * Or with static linking:
 *   gcc -o c_example c_example.c ../target/release/libcassettedb.a -lpthread -ldl
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../cassette.h"

int main(void) {
    const char *db_path = "example.cassette";
    
    printf("CassetteDB C FFI Example\n");
    printf("========================\n\n");
    
    /* Open database */
    CassetteDB *db = cassette_db_open(db_path);
    if (!db) {
        char *err = cassette_last_error();
        fprintf(stderr, "Failed to open database: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
        return 1;
    }
    printf("Database opened: %s\n", db_path);
    
    /* Insert a document */
    char *doc_id = cassette_insert(db, "{\"name\":\"Alice\",\"age\":30,\"city\":\"NYC\"}");
    if (!doc_id) {
        char *err = cassette_last_error();
        fprintf(stderr, "Insert failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
        cassette_db_close(db);
        return 1;
    }
    printf("Inserted document ID: %s\n", doc_id);
    
    /* Retrieve the document */
    char *doc = cassette_get(db, doc_id);
    if (doc) {
        printf("Retrieved document: %s\n", doc);
        cassette_free_string(doc);
    } else {
        char *err = cassette_last_error();
        fprintf(stderr, "Get failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
    }
    
    /* Update the document */
    int rc = cassette_update(db, doc_id, "{\"name\":\"Alice\",\"age\":31,\"city\":\"LA\"}");
    if (rc == 0) {
        printf("Document updated successfully\n");
    } else {
        char *err = cassette_last_error();
        fprintf(stderr, "Update failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
    }
    
    /* Query */
    char *results = cassette_query(db, "age > 25");
    if (results) {
        printf("Query results: %s\n", results);
        cassette_free_string(results);
    } else {
        char *err = cassette_last_error();
        fprintf(stderr, "Query failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
    }
    
    /* Dump all documents */
    char *dump = cassette_dump(db);
    if (dump) {
        printf("Database dump: %s\n", dump);
        cassette_free_string(dump);
    }
    
    /* Compact */
    rc = cassette_compact(db);
    if (rc == 0) {
        printf("Database compacted successfully\n");
    } else {
        char *err = cassette_last_error();
        fprintf(stderr, "Compact failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
    }
    
    /* Delete document */
    rc = cassette_delete(db, doc_id);
    if (rc == 0) {
        printf("Document deleted successfully\n");
    } else {
        char *err = cassette_last_error();
        fprintf(stderr, "Delete failed: %s\n", err ? err : "unknown error");
        cassette_free_string(err);
    }
    
    /* Cleanup */
    cassette_free_string(doc_id);
    cassette_db_close(db);
    
    printf("\nExample completed successfully!\n");
    return 0;
}
