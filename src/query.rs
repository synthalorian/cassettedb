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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueryResult {
    pub documents: Vec<Document>,
    pub count: usize,
}

/// Detailed parse error with context.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: Option<usize>,
    pub context: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(pos) = self.position {
            write!(f, " at position {}", pos)?;
        }
        if !self.context.is_empty() {
            write!(f, " near '{}'", self.context)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

impl Query {
    /// Parse a tiny query DSL string.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(CassetteError::InvalidQuery(
                ParseError {
                    message: "Empty query string".to_string(),
                    position: Some(0),
                    context: "".to_string(),
                }
                .to_string(),
            ));
        }
        if input == "*" || input == "$" {
            return Ok(Query::All);
        }

        // search("...")
        if input.starts_with("search(") {
            let inner = input.strip_prefix("search(").ok_or_else(|| {
                CassetteError::InvalidQuery(
                    ParseError {
                        message: "Invalid search(...) expression".to_string(),
                        position: Some(0),
                        context: input.to_string(),
                    }
                    .to_string(),
                )
            })?;
            let inner = inner.strip_suffix(")").ok_or_else(|| {
                CassetteError::InvalidQuery(
                    ParseError {
                        message: "Unclosed search(...) expression — missing closing ')'".to_string(),
                        position: Some(input.len() - 1),
                        context: input.to_string(),
                    }
                    .to_string(),
                )
            })?;
            let term = inner.trim().trim_matches('"').trim_matches('\'');
            if term.is_empty() {
                return Err(CassetteError::InvalidQuery(
                    ParseError {
                        message: "Empty search term in search(...)".to_string(),
                        position: Some(7),
                        context: input[..20.min(input.len())].to_string(),
                    }
                    .to_string(),
                ));
            }
            return Ok(Query::Search(term.to_string()));
        }

        // and / or
        if let Some(pos) = find_balanced(input, " and ") {
            let left = &input[..pos];
            let right = &input[pos + 5..];
            if left.trim().is_empty() {
                return Err(CassetteError::InvalidQuery(
                    ParseError {
                        message: "Missing left-hand side of 'and' expression".to_string(),
                        position: Some(pos),
                        context: input.to_string(),
                    }
                    .to_string(),
                ));
            }
            if right.trim().is_empty() {
                return Err(CassetteError::InvalidQuery(
                    ParseError {
                        message: "Missing right-hand side of 'and' expression".to_string(),
                        position: Some(pos + 5),
                        context: input.to_string(),
                    }
                    .to_string(),
                ));
            }
            return Ok(Query::And(
                Box::new(Query::parse(left)?),
                Box::new(Query::parse(right)?),
            ));
        }
        if let Some(pos) = find_balanced(input, " or ") {
            let left = &input[..pos];
            let right = &input[pos + 4..];
            if left.trim().is_empty() {
                return Err(CassetteError::InvalidQuery(
                    ParseError {
                        message: "Missing left-hand side of 'or' expression".to_string(),
                        position: Some(pos),
                        context: input.to_string(),
                    }
                    .to_string(),
                ));
            }
            if right.trim().is_empty() {
                return Err(CassetteError::InvalidQuery(
                    ParseError {
                        message: "Missing right-hand side of 'or' expression".to_string(),
                        position: Some(pos + 4),
                        context: input.to_string(),
                    }
                    .to_string(),
                ));
            }
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
                if left.is_empty() {
                    return Err(CassetteError::InvalidQuery(
                        ParseError {
                            message: format!(
                                "Missing left-hand side of '{}' comparison",
                                op
                            ),
                            position: Some(pos),
                            context: input.to_string(),
                        }
                        .to_string(),
                    ));
                }
                if right.is_empty() {
                    return Err(CassetteError::InvalidQuery(
                        ParseError {
                            message: format!(
                                "Missing right-hand side of '{}' comparison",
                                op
                            ),
                            position: Some(pos + op.len()),
                            context: input.to_string(),
                        }
                        .to_string(),
                    ));
                }
                let path = parse_path(left)?;
                if variant == "eq" {
                    let value = parse_value(right)?;
                    return Ok(Query::Eq { path, value });
                } else {
                    let num = parse_number(right).map_err(|_e| {
                        CassetteError::InvalidQuery(
                            ParseError {
                                message: format!(
                                    "Expected numeric value for '{}' comparison, got '{}'",
                                    op, right
                                ),
                                position: Some(pos + op.len()),
                                context: right.to_string(),
                            }
                            .to_string(),
                        )
                    })?;
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

        Err(CassetteError::InvalidQuery(
            ParseError {
                message: format!(
                    "Unable to parse query: '{}'",
                    input
                ),
                position: Some(0),
                context: if input.len() > 40 {
                    format!("{}...", &input[..40])
                } else {
                    input.to_string()
                },
            }
            .to_string(),
        ))
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
                resolve_number(&doc.data, path).is_some_and(|v| v > *value)
            }
            Query::Lt { path, value } => {
                resolve_number(&doc.data, path).is_some_and(|v| v < *value)
            }
            Query::Gte { path, value } => {
                resolve_number(&doc.data, path).is_some_and(|v| v >= *value)
            }
            Query::Lte { path, value } => {
                resolve_number(&doc.data, path).is_some_and(|v| v <= *value)
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
        return Err(CassetteError::InvalidQuery(
            ParseError {
                message: "Empty field path".to_string(),
                position: Some(0),
                context: s.to_string(),
            }
            .to_string(),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_error_messages() {
        let err = Query::parse("").unwrap_err().to_string();
        assert!(err.contains("Empty query string"), "Expected 'Empty query string' in: {}", err);

        let err = Query::parse("search(").unwrap_err().to_string();
        assert!(err.contains("Unclosed search(...)"), "Expected 'Unclosed search(...)' in: {}", err);

        let err = Query::parse("and foo").unwrap_err().to_string();
        assert!(err.contains("Unable to parse query"), "Expected 'Unable to parse query' in: {}", err);

        let err = Query::parse("age > ").unwrap_err().to_string();
        assert!(err.contains("Missing right-hand side"), "Expected 'Missing right-hand side' in: {}", err);

        let err = Query::parse("age > abc").unwrap_err().to_string();
        assert!(err.contains("Expected numeric value"), "Expected 'Expected numeric value' in: {}", err);
    }

    #[test]
    fn test_valid_queries_still_work() {
        let q = Query::parse("*").unwrap();
        assert_eq!(q, Query::All);

        let q = Query::parse("search(\"hello\")").unwrap();
        assert_eq!(q, Query::Search("hello".to_string()));

        let q = Query::parse("age > 25").unwrap();
        assert!(matches!(q, Query::Gt { path, value } if path == vec!["age"] && value == 25.0));

        let q = Query::parse("age >= 25 and search(\"test\")").unwrap();
        assert!(matches!(q, Query::And(_, _)));
    }
}
