use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::db::{Cassette, Collection, Document, InvertedIndex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub collection: String,
    pub doc_id: String,
    pub local: Value,
    pub remote: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncSummary {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub conflicts: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub summary: SyncSummary,
    pub conflicts: Vec<Conflict>,
}

impl SyncResult {
    pub fn empty() -> Self {
        Self {
            summary: SyncSummary::default(),
            conflicts: Vec::new(),
        }
    }
}

/// Merge `remote` cassette into `local`, detecting conflicts when both sides
/// have independently modified the same document.
pub fn sync_into(local: &mut Cassette, remote: &Cassette) -> Result<SyncResult> {
    let mut result = SyncResult::empty();

    for (coll_name, remote_coll) in &remote.collections {
        let local_coll = local
            .collections
            .entry(coll_name.clone())
            .or_insert_with(|| empty_collection(coll_name));

        for remote_doc in &remote_coll.documents {
            match local_coll.documents.iter_mut().find(|d| d.id == remote_doc.id) {
                None => {
                    if !remote_doc.deleted {
                        add_document(local_coll, remote_doc)?;
                        result.summary.added += 1;
                    }
                }
                Some(local_doc) => {
                    let local_updated = parse_time(&local_doc.updated_at);
                    let remote_updated = parse_time(&remote_doc.updated_at);

                    if remote_doc.deleted && local_doc.deleted {
                        result.summary.unchanged += 1;
                    } else if remote_doc.deleted && remote_updated > local_updated {
                        local_doc.deleted = true;
                        local_doc.updated_at = remote_doc.updated_at.clone();
                        result.summary.deleted += 1;
                    } else if local_doc.deleted && local_updated > remote_updated {
                        result.summary.unchanged += 1;
                    } else if local_doc.deleted && remote_updated > local_updated {
                        local_doc.deleted = false;
                        local_doc.data = remote_doc.data.clone();
                        local_doc.updated_at = remote_doc.updated_at.clone();
                        Cassette::reindex_document(&mut local_coll.inverted_index, local_doc);
                        result.summary.updated += 1;
                    } else if local_doc.data == remote_doc.data
                        && local_doc.deleted == remote_doc.deleted
                    {
                        result.summary.unchanged += 1;
                    } else if remote_updated > local_updated {
                        local_doc.data = remote_doc.data.clone();
                        local_doc.updated_at = remote_doc.updated_at.clone();
                        Cassette::reindex_document(&mut local_coll.inverted_index, local_doc);
                        result.summary.updated += 1;
                    } else if local_updated > remote_updated {
                        result.summary.unchanged += 1;
                    } else {
                        result.conflicts.push(Conflict {
                            collection: coll_name.clone(),
                            doc_id: local_doc.id.clone(),
                            local: local_doc.data.clone(),
                            remote: remote_doc.data.clone(),
                        });
                        result.summary.conflicts += 1;
                    }
                }
            }
        }
    }

    Ok(result)
}

fn empty_collection(name: &str) -> Collection {
    Collection {
        name: name.to_string(),
        documents: Vec::new(),
        indexes: HashMap::new(),
        inverted_index: InvertedIndex::new(),
        secondary_indexes: HashMap::new(),
    }
}

fn add_document(coll: &mut Collection, doc: &Document) -> Result<()> {
    let new_doc = Document {
        id: doc.id.clone(),
        data: doc.data.clone(),
        created_at: doc.created_at.clone(),
        updated_at: doc.updated_at.clone(),
        deleted: doc.deleted,
    };
    Cassette::reindex_document(&mut coll.inverted_index, &new_doc);
    coll.documents.push(new_doc);
    Ok(())
}

fn parse_time(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::DateTime::UNIX_EPOCH)
}

impl Cassette {
    pub fn reindex_document(inverted_index: &mut InvertedIndex, doc: &Document) {
        use regex::Regex;
        let re = Regex::new(r"[^a-zA-Z0-9]+").unwrap();
        if let Some(obj) = doc.data.as_object() {
            for (field, value) in obj {
                if let Some(text) = value.as_str() {
                    let tokens: Vec<String> = re
                        .split(text)
                        .filter(|t| !t.is_empty())
                        .map(|t| t.to_lowercase())
                        .collect();
                    let field_index = inverted_index.entry(field.clone()).or_default();
                    for token in tokens {
                        let ids = field_index.entry(token).or_default();
                        if !ids.contains(&doc.id) {
                            ids.push(doc.id.clone());
                        }
                    }
                }
            }
        }
    }
}
