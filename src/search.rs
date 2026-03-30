use std::sync::Arc;

use crate::model::node::{JsonDocument, JsonValue, NodeId};

/// A single search match.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub node_id: NodeId,
    #[allow(dead_code)]
    pub match_in: MatchLocation,
}

/// Where the match was found.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum MatchLocation {
    Key(Arc<str>),
    Value,
}

/// Search options.
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub search_keys: bool,
    pub search_values: bool,
    /// Stop after this many hits (0 = unlimited).
    pub max_hits: usize,
    /// Treat the query as a regular expression.
    pub regex_mode: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            search_keys: true,
            search_values: true,
            max_hits: 1000,
            regex_mode: false,
        }
    }
}

/// Search the document for nodes matching the query string.
pub fn search(doc: &JsonDocument, query: &str, options: &SearchOptions) -> Vec<SearchHit> {
    if query.is_empty() {
        return Vec::new();
    }

    // Compile the regex once up front; if the pattern is invalid, return no results.
    let compiled_regex: Option<regex::Regex> = if options.regex_mode {
        regex::RegexBuilder::new(query)
            .case_insensitive(!options.case_sensitive)
            .build()
            .ok()
    } else {
        None
    };

    // For invalid regex in regex_mode, compiled_regex is None — bail out early.
    if options.regex_mode && compiled_regex.is_none() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    let query_lower = if options.case_sensitive || options.regex_mode {
        query.to_string()
    } else {
        query.to_lowercase()
    };

    search_node(
        doc,
        doc.root(),
        &query_lower,
        options,
        compiled_regex.as_ref(),
        &mut hits,
    );
    hits
}

fn search_node(
    doc: &JsonDocument,
    root_id: NodeId,
    query: &str,
    options: &SearchOptions,
    compiled_regex: Option<&regex::Regex>,
    hits: &mut Vec<SearchHit>,
) {
    // Iterative DFS using an explicit stack — no risk of stack overflow on deep JSON.
    let mut stack: Vec<NodeId> = vec![root_id];

    while let Some(id) = stack.pop() {
        // Early termination when hit limit is reached.
        if options.max_hits > 0 && hits.len() >= options.max_hits {
            break;
        }

        let node = doc.node(id);

        // Check if the value itself matches.
        if options.search_values {
            let value_matches = match &node.value {
                JsonValue::Null => matches_value("null", query, options, compiled_regex),
                JsonValue::Bool(b) => {
                    let s = if *b { "true" } else { "false" };
                    matches_value(s, query, options, compiled_regex)
                }
                JsonValue::Number(n) => {
                    matches_value(&n.to_string(), query, options, compiled_regex)
                }
                JsonValue::String(s) => matches_value(s, query, options, compiled_regex),
                JsonValue::Array(_) | JsonValue::Object(_) => false,
            };
            if value_matches {
                hits.push(SearchHit {
                    node_id: id,
                    match_in: MatchLocation::Value,
                });
            }
        }

        // Push children, checking object keys along the way.
        // `doc` is an immutable borrow and `hits` is a mutable borrow of a
        // separate allocation — no conflict; no clones required.
        match &node.value {
            JsonValue::Array(children) => {
                // Push in reverse so we process left-to-right after popping.
                for i in (0..children.len()).rev() {
                    stack.push(children[i]);
                }
            }
            JsonValue::Object(entries) => {
                for i in (0..entries.len()).rev() {
                    let (key, child_id) = &entries[i];
                    // If the key matches, emit a Key hit and skip the value
                    // check for this child to prevent duplicate hits when both
                    // the key and the value satisfy the query.
                    if options.search_keys && matches_value(key, query, options, compiled_regex) {
                        hits.push(SearchHit {
                            node_id: *child_id,
                            match_in: MatchLocation::Key(Arc::clone(key)),
                        });
                        // Child is already recorded; do not push it onto the
                        // stack for a second (value) match.
                        continue;
                    }
                    stack.push(*child_id);
                }
            }
            _ => {}
        }
    }
}

/// Returns true if `haystack` satisfies the search query according to the given options.
///
/// When a pre-compiled regex is provided it is used directly (regex mode).
/// Otherwise plain substring matching with optional case folding is used.
#[inline]
fn matches_value(
    haystack: &str,
    query: &str,
    options: &SearchOptions,
    compiled_regex: Option<&regex::Regex>,
) -> bool {
    if let Some(re) = compiled_regex {
        re.is_match(haystack)
    } else if options.case_sensitive {
        haystack.contains(query)
    } else {
        haystack.to_lowercase().contains(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::DocumentBuilder;
    use std::time::Duration;

    fn test_doc() -> JsonDocument {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "name": "Alice",
                "age": 30,
                "email": "alice@example.com",
                "items": ["apple", "banana"]
            }"#,
        )
        .unwrap();
        DocumentBuilder::from_serde_value(json, None, 0, Duration::ZERO)
    }

    #[test]
    fn search_finds_string_value() {
        let doc = test_doc();
        let hits = search(&doc, "alice", &SearchOptions::default());
        assert!(hits.len() >= 1);
    }

    #[test]
    fn search_finds_key() {
        let doc = test_doc();
        let hits = search(&doc, "email", &SearchOptions::default());
        assert!(
            hits.iter()
                .any(|h| matches!(h.match_in, MatchLocation::Key(_)))
        );
    }

    #[test]
    fn search_case_insensitive() {
        let doc = test_doc();
        let hits = search(&doc, "ALICE", &SearchOptions::default());
        assert!(!hits.is_empty());
    }

    #[test]
    fn search_case_sensitive() {
        let doc = test_doc();
        let opts = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let hits = search(&doc, "ALICE", &opts);
        assert!(hits.is_empty());
    }

    #[test]
    fn search_empty_query() {
        let doc = test_doc();
        let hits = search(&doc, "", &SearchOptions::default());
        assert!(hits.is_empty());
    }

    #[test]
    fn search_number() {
        let doc = test_doc();
        let hits = search(&doc, "30", &SearchOptions::default());
        assert!(!hits.is_empty());
    }

    #[test]
    fn search_regex_basic() {
        let doc = test_doc();
        let opts = SearchOptions {
            regex_mode: true,
            ..Default::default()
        };
        // Matches "alice@example.com" via a simple pattern.
        let hits = search(&doc, r"alice@\w+\.\w+", &opts);
        assert!(!hits.is_empty());
    }

    #[test]
    fn search_regex_case_insensitive() {
        let doc = test_doc();
        let opts = SearchOptions {
            regex_mode: true,
            case_sensitive: false,
            ..Default::default()
        };
        let hits = search(&doc, "ALICE", &opts);
        assert!(!hits.is_empty());
    }

    #[test]
    fn search_regex_invalid_returns_empty() {
        let doc = test_doc();
        let opts = SearchOptions {
            regex_mode: true,
            ..Default::default()
        };
        // "[invalid" is not a valid regex — should return empty rather than panic.
        let hits = search(&doc, "[invalid", &opts);
        assert!(hits.is_empty());
    }
}
