"""Python bindings for CassetteDB.

This module provides a thin ctypes wrapper around the CassetteDB C FFI.
Load the shared `libcassettedb` library and use the `CassetteDB` class
to open, query, and modify single-file JSON document databases.

Example:
    >>> from cassettedb import CassetteDB
    >>> db = CassetteDB("mydb.cassette")
    >>> doc_id = db.insert({"name": "Ada", "age": 36})
    >>> db.get(doc_id)
    {'name': 'Ada', 'age': 36}
    >>> db.close()
"""

from __future__ import annotations

import ctypes
import json
import os
import platform
import sys
from pathlib import Path
from types import TracebackType
from typing import Any, Optional, Type

__all__ = ["CassetteDB", "CassetteDBError", "load_library"]


class CassetteDBError(Exception):
    """Raised when CassetteDB reports an error."""

    pass


def _library_name() -> str:
    system = platform.system()
    if system == "Darwin":
        return "libcassettedb.dylib"
    if system == "Windows":
        return "libcassettedb.dll"
    return "libcassettedb.so"


def load_library(path: Optional[str] = None) -> ctypes.CDLL:
    """Load the `libcassettedb` shared library.

    If `path` is not provided, the following locations are tried in order:
    1. A sibling `target/release` directory (typical Rust build output).
    2. A sibling `target/debug` directory.
    3. The platform search path (LD_LIBRARY_PATH, PATH, etc.).
    """
    if path:
        return ctypes.CDLL(path)

    here = Path(__file__).resolve().parent
    candidates = [
        here / ".." / ".." / ".." / "target" / "release" / _library_name(),
        here / ".." / ".." / ".." / "target" / "debug" / _library_name(),
        here / _library_name(),
        Path(_library_name()),
    ]
    for candidate in candidates:
        candidate = candidate.resolve()
        if candidate.exists():
            return ctypes.CDLL(str(candidate))

    # Fall back to letting the OS loader resolve it.
    return ctypes.CDLL(_library_name())


class CassetteDB:
    """Pythonic wrapper around the CassetteDB C API.

    All methods raise :class:`CassetteDBError` on failure unless otherwise
    noted. Strings returned by the underlying C library are automatically
    freed.
    """

    _lib: Optional[ctypes.CDLL] = None

    def __init__(self, path: str, *, library_path: Optional[str] = None) -> None:
        """Open (or create) a database at `path`.

        Args:
            path: Filesystem path to the `.cassette` database file.
            library_path: Optional explicit path to the `libcassettedb`
                shared library. When omitted, a heuristic search is used.
        """
        self._lib = load_library(library_path)

        # Declare signatures for a bit of runtime safety.
        self._lib.cassette_db_open.argtypes = [ctypes.c_char_p]
        self._lib.cassette_db_open.restype = ctypes.c_void_p
        self._lib.cassette_db_close.argtypes = [ctypes.c_void_p]
        self._lib.cassette_db_close.restype = None
        self._lib.cassette_insert.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self._lib.cassette_insert.restype = ctypes.c_char_p
        self._lib.cassette_get.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self._lib.cassette_get.restype = ctypes.c_char_p
        self._lib.cassette_update.argtypes = [
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.c_char_p,
        ]
        self._lib.cassette_update.restype = ctypes.c_int
        self._lib.cassette_delete.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self._lib.cassette_delete.restype = ctypes.c_int
        self._lib.cassette_query.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self._lib.cassette_query.restype = ctypes.c_char_p
        self._lib.cassette_dump.argtypes = [ctypes.c_void_p]
        self._lib.cassette_dump.restype = ctypes.c_char_p
        self._lib.cassette_compact.argtypes = [ctypes.c_void_p]
        self._lib.cassette_compact.restype = ctypes.c_int
        self._lib.cassette_last_error.argtypes = []
        self._lib.cassette_last_error.restype = ctypes.c_char_p
        self._lib.cassette_free_string.argtypes = [ctypes.c_char_p]
        self._lib.cassette_free_string.restype = None

        c_path = path.encode("utf-8")
        self._db = self._lib.cassette_db_open(c_path)
        if not self._db:
            raise CassetteDBError(self._last_error())

    def _last_error(self) -> str:
        err_ptr = self._lib.cassette_last_error()
        try:
            if err_ptr:
                return ctypes.cast(err_ptr, ctypes.c_char_p).value.decode("utf-8")
        finally:
            self._lib.cassette_free_string(err_ptr)
        return "unknown error"

    def _take_string(self, ptr: ctypes.c_char_p) -> Optional[str]:
        if not ptr:
            return None
        try:
            raw = ctypes.cast(ptr, ctypes.c_char_p).value
            return raw.decode("utf-8") if raw else None
        finally:
            self._lib.cassette_free_string(ptr)

    def insert(self, document: Any) -> str:
        """Insert a JSON-serializable document and return its ID."""
        json_bytes = json.dumps(document, ensure_ascii=False).encode("utf-8")
        id_ptr = self._lib.cassette_insert(self._db, json_bytes)
        if not id_ptr:
            raise CassetteDBError(self._last_error())
        return self._take_string(id_ptr)  # type: ignore[arg-return]

    def get(self, doc_id: str) -> Optional[Any]:
        """Retrieve a document by ID, or ``None`` if not found."""
        doc_ptr = self._lib.cassette_get(self._db, doc_id.encode("utf-8"))
        raw = self._take_string(doc_ptr)
        if raw is None:
            return None
        return json.loads(raw)

    def update(self, doc_id: str, document: Any) -> None:
        """Replace the document identified by `doc_id`."""
        json_bytes = json.dumps(document, ensure_ascii=False).encode("utf-8")
        rc = self._lib.cassette_update(
            self._db, doc_id.encode("utf-8"), json_bytes
        )
        if rc != 0:
            raise CassetteDBError(self._last_error())

    def delete(self, doc_id: str) -> None:
        """Delete a document by ID."""
        rc = self._lib.cassette_delete(self._db, doc_id.encode("utf-8"))
        if rc != 0:
            raise CassetteDBError(self._last_error())

    def query(self, query: str) -> list[Any]:
        """Execute a CassetteDB query and return the matching documents."""
        res_ptr = self._lib.cassette_query(self._db, query.encode("utf-8"))
        raw = self._take_string(res_ptr)
        if raw is None:
            raise CassetteDBError(self._last_error())
        return json.loads(raw)

    def dump(self) -> list[Any]:
        """Return all documents in the database."""
        dump_ptr = self._lib.cassette_dump(self._db)
        raw = self._take_string(dump_ptr)
        if raw is None:
            raise CassetteDBError(self._last_error())
        return json.loads(raw)

    def compact(self) -> None:
        """Rewrite the main database file and truncate the WAL."""
        rc = self._lib.cassette_compact(self._db)
        if rc != 0:
            raise CassetteDBError(self._last_error())

    def close(self) -> None:
        """Close the database and release native resources."""
        if self._db and self._lib:
            self._lib.cassette_db_close(self._db)
            self._db = None  # type: ignore[assignment]

    def __enter__(self) -> "CassetteDB":
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_val: Optional[BaseException],
        exc_tb: Optional[TracebackType],
    ) -> None:
        self.close()

    def __del__(self) -> None:
        self.close()
