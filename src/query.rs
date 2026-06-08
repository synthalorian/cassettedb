//! JSONPath-like query engine.
//!
//! Supports a tiny subset:
//!   - `$.field` or `field`      : exact field match
//!   - `$.field.subfield`        : nested field
//!   - `$.field == value`        : equality
//!   - `$.field > value` etc.    : ordering (numbers)
//!   - `search("text")`          : full-text search
//!   - `and`, `or`               : logical combinators

use crate::document::Document;
use crate::error::{CassetteError, Result};
use serde_json::Value;

/// Parsed query.
#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    All,
    Eq { path: Vec<String>, value: Value },
    Gt { path: Vec<String>, value: f64 },
    Lt { path: Vec<String>, value: f64 },
    Gte { path: Vec<String>, value: f64 },
    Lte { path: Vec<String>, value: f64 },
    Search(String),
    And(Box<Query>, Box<Query>),
    Or(Box<Query>, Box<Query>),
}

/// Result of a query.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub documents: Vec<Document>,
    pub count: usize,
}

impl Query {
    /// Parse a tiny query DSL string.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input == "*" || input == "$" {
            return Ok(Query::All);
        }

        // search("...")
        if let Some(inner) = input.strip_prefix("search(") {
            let inner = inner.strip_suffix(")").unwrap_or(inner);
            let term = inner.trim().trim_matches('"').trim_matches('\'');
            return Ok(Query::Search(term.to_string()));
        }

        // and / or
        if let Some(pos) = find_balanced(input, " and ") {
            let left = &input[..pos];
            let right = &input[pos + 5..];
            return Ok(Query::And(
                Box::new(Query::parse(left)?),
                Box::new(Query::parse(right)?),
            ));
        }
        if let Some(pos) = find_balanced(input, " or ") {
            let left = &input[..pos];
            let right = &input[pos + 4..];
            return Ok(Query::Or(
                Box::new(Query::parse(left)?),
                Box::new(Query::parse(right)?),
            ));
        }

        // comparisons
        for (op, variant) in [
            (">=", "gte"),
            ("<=", "lte"),
            (">", "gt"),
            ("<", "lt"),
            ("==", "eq"),
        ] {
            if let Some(pos) = find_balanced(input, op) {
                let left = input[..pos].trim();
                let right = input[pos + op.len()..].trim();
                let path = parse_path(left)?;
                if variant == "eq" {
                    let value = parse_value(right)?;
                    return Ok(Query::Eq { path, value });
                } else {
                    let num = parse_number(right)?;
                    return Ok(match variant {
                        "gt" => Query::Gt { path, value: num },
                        "lt" => Query::Lt { path, value: num },
                        "gte" => Query::Gte { path, value: num },
                        "lte" => Query::Lte { path, value: num },
                        _ => unreachable!(),
                    });
                }
            }
        }

        Err(CassetteError::InvalidQuery(format!(
            "Unable to parse query: {}",
            input
        )))
    }

    /// Evaluate the query against a collection of documents.
    pub fn execute(
        &self,
        docs: &[Document],
        ft_index: &crate::index::InvertedIndex,
    ) -> QueryResult {
        let matched: Vec<Document> = docs
            .iter()
            .filter(|d| self.matches(d, ft_index))
            .cloned()
            .collect();
        let count = matched.len();
        QueryResult {
            documents: matched,
            count,
        }
    }

    fn matches(&self, doc: &Document, ft_index: &crate::index::InvertedIndex) -> bool {
        match self {
            Query::All => true,
            Query::Eq { path, value } => {
                let got = resolve_path(&doc.data, path);
                got == Some(value.clone())
            }
            Query::Gt { path, value } => {
                resolve_number(&doc.data, path).map_or(false, |v| v > *value)
            }
            Query::Lt { path, value } => {
                resolve_number(&doc.data, path).map_or(false, |v| v < *value)
            }
            Query::Gte { path, value } => {
                resolve_number(&doc.data, path).map_or(false, |v| v >= *value)
            }
            Query::Lte { path, value } => {
                resolve_number(&doc.data, path).map_or(false, |v| v <= *value)
            }
            Query::Search(term) => ft_index.search(term).contains(&doc.id),
            Query::And(a, b) => a.matches(doc, ft_index) && b.matches(doc, ft_index),
            Query::Or(a, b) => a.matches(doc, ft_index) || b.matches(doc, ft_index),
        }
    }
}

fn parse_path(s: &str) -> Result<Vec<String>> {
    let s = s.trim();
    let s = s.strip_prefix("$").unwrap_or(s);
    let s = s.strip_prefix(".").unwrap_or(s);
    if s.is_empty() {
        return Err(CassetteError::InvalidQuery("Empty path".into()));
    }
    Ok(s.split('.').map(|p| p.to_string()).collect())
}

fn parse_value(s: &str) -> Result<Value> {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return Ok(Value::String(s[1..s.len() - 1].to_string()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Value::Number(n.into()));
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return Ok(Value::Number(num));
        }
    }
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }
    if s == "null" {
        return Ok(Value::Null);
    }
    Ok(Value::String(s.to_string()))
}

fn parse_number(s: &str) -> Result<f64> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| CassetteError::InvalidQuery(format!("Not a number: {}", s)))
}

fn resolve_path(value: &Value, path: &[String]) -> Option<Value> {
    let mut current = value;
    for segment in path {
        match current {
            Value::Object(map) => current = map.get(segment)?,
            _ => return None,
        }
    }
    Some(current.clone())
}

fn resolve_number(value: &Value, path: &[String]) -> Option<f64> {
    let v = resolve_path(value, path)?;
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Find a substring that is not inside parentheses.
fn find_balanced(haystack: &str, needle: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in haystack.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && haystack[i..].starts_with(needle) {
            return Some(i);
        }
    }
    None
}
