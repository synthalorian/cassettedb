use pyo3::prelude::*;
use serde_json::Value;
use std::path::PathBuf;

use crate::db::Cassette;

#[pyclass(name = "CassetteDB")]
pub struct PyCassetteDB {
    cassette: Cassette,
    path: PathBuf,
}

#[pymethods]
impl PyCassetteDB {
    #[new]
    #[pyo3(signature = (path=None))]
    pub fn new(path: Option<&str>) -> PyResult<Self> {
        let path = path.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data.cassette"));
        let cassette = Cassette::open(&path).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { cassette, path })
    }

    fn insert(&mut self, collection: &str, doc: &str) -> PyResult<String> {
        let value: Value = serde_json::from_str(doc)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let id = self.cassette.insert(collection, value)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        self.save()?;
        Ok(id)
    }

    fn query(&self, collection: &str, filter: &str) -> PyResult<Vec<String>> {
        let results = self.cassette.query(collection, filter)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        results.into_iter()
            .map(|doc| serde_json::to_string(doc).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string())))
            .collect()
    }

    fn query_jsonpath(&self, collection: &str, path: &str) -> PyResult<Vec<String>> {
        let results = self.cassette.query_jsonpath(collection, path)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        results.into_iter()
            .map(|doc| serde_json::to_string(doc).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string())))
            .collect()
    }

    fn search(&self, collection: &str, query: &str) -> PyResult<Vec<String>> {
        let results = self.cassette.search(collection, query)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        results.into_iter()
            .map(|doc| serde_json::to_string(doc).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string())))
            .collect()
    }

    fn get(&self, collection: &str, id: &str) -> PyResult<Option<String>> {
        match self.cassette.get(collection, id) {
            Some(doc) => Ok(Some(serde_json::to_string(doc)
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?)),
            None => Ok(None),
        }
    }

    fn scan(&self, collection: &str) -> PyResult<Vec<String>> {
        let results = self.cassette.scan(collection)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        results.into_iter()
            .map(|doc| serde_json::to_string(doc).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string())))
            .collect()
    }

    fn update(&mut self, collection: &str, id: &str, doc: &str) -> PyResult<bool> {
        let value: Value = serde_json::from_str(doc)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let ok = self.cassette.update(collection, id, value)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        if ok {
            self.save()?;
        }
        Ok(ok)
    }

    fn delete(&mut self, collection: &str, id: &str) -> PyResult<bool> {
        let ok = self.cassette.delete(collection, id)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        if ok {
            self.save()?;
        }
        Ok(ok)
    }

    fn collections(&self) -> PyResult<Vec<String>> {
        Ok(self.cassette.collections().into_iter().cloned().collect())
    }

    fn compact(&mut self) -> PyResult<usize> {
        let removed = self.cassette.compact()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        self.save()?;
        Ok(removed)
    }

    fn save(&mut self) -> PyResult<()> {
        self.cassette.save(&self.path)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }
}
