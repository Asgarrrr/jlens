use serde_json::Value;

use crate::diff::{DiffNode, DiffResult, DiffStats, DiffStatus};

/// Compute the structural diff between two JSON values.
pub fn diff(left: &Value, right: &Value) -> DiffResult {
    let mut stats = DiffStats::default();
    let root = diff_values(left, right, None, None, 0, &mut stats);
    DiffResult { root, stats }
}

fn diff_values(
    left: &Value,
    right: &Value,
    key: Option<String>,
    array_index: Option<usize>,
    depth: u16,
    stats: &mut DiffStats,
) -> DiffNode {
    match (left, right) {
        // Both objects — recurse per key
        (Value::Object(left_map), Value::Object(right_map)) => {
            let mut children = Vec::new();

            // Keys only in left → Removed
            for (k, lv) in left_map {
                if !right_map.contains_key(k) {
                    let child = removed_subtree(lv, Some(k.clone()), None, depth + 1, stats);
                    children.push(child);
                }
            }

            // Keys only in right → Added
            for (k, rv) in right_map {
                if !left_map.contains_key(k) {
                    let child = added_subtree(rv, Some(k.clone()), None, depth + 1, stats);
                    children.push(child);
                }
            }

            // Keys in both → recurse
            for (k, lv) in left_map {
                if let Some(rv) = right_map.get(k) {
                    let child = diff_values(lv, rv, Some(k.clone()), None, depth + 1, stats);
                    children.push(child);
                }
            }

            // Sort for stable output: removed first, then added, then by key
            children.sort_by(|a, b| {
                let order = |n: &DiffNode| match n.status {
                    DiffStatus::Removed => 0,
                    DiffStatus::Added => 1,
                    DiffStatus::Modified => 2,
                    DiffStatus::Unchanged => 3,
                };
                order(a)
                    .cmp(&order(b))
                    .then_with(|| a.key.cmp(&b.key))
            });

            // The object container itself is Unchanged unless all children are Unchanged
            let status = if children.iter().all(|c| c.status == DiffStatus::Unchanged) {
                stats.unchanged += 1;
                DiffStatus::Unchanged
            } else {
                // Container changed — don't count it as a leaf stat; children track their own
                DiffStatus::Modified
            };

            DiffNode {
                status,
                key,
                array_index,
                left: Some(left.clone()),
                right: Some(right.clone()),
                children,
                depth,
            }
        }

        // Both arrays — index-based comparison
        (Value::Array(left_arr), Value::Array(right_arr)) => {
            let left_len = left_arr.len();
            let right_len = right_arr.len();
            let common = left_len.min(right_len);
            let mut children = Vec::new();

            // Items in both — recurse
            for i in 0..common {
                let child = diff_values(&left_arr[i], &right_arr[i], None, Some(i), depth + 1, stats);
                children.push(child);
            }

            // Extra items in left → Removed
            for i in common..left_len {
                let child = removed_subtree(&left_arr[i], None, Some(i), depth + 1, stats);
                children.push(child);
            }

            // Extra items in right → Added
            for i in common..right_len {
                let child = added_subtree(&right_arr[i], None, Some(i), depth + 1, stats);
                children.push(child);
            }

            let status = if children.iter().all(|c| c.status == DiffStatus::Unchanged) {
                stats.unchanged += 1;
                DiffStatus::Unchanged
            } else {
                DiffStatus::Modified
            };

            DiffNode {
                status,
                key,
                array_index,
                left: Some(left.clone()),
                right: Some(right.clone()),
                children,
                depth,
            }
        }

        // Same type scalar: compare directly
        (Value::Null, Value::Null) => {
            stats.unchanged += 1;
            DiffNode {
                status: DiffStatus::Unchanged,
                key,
                array_index,
                left: Some(left.clone()),
                right: Some(right.clone()),
                children: Vec::new(),
                depth,
            }
        }
        (Value::Bool(a), Value::Bool(b)) => {
            if a == b {
                stats.unchanged += 1;
                DiffNode {
                    status: DiffStatus::Unchanged,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            } else {
                stats.modified += 1;
                DiffNode {
                    status: DiffStatus::Modified,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            }
        }
        (Value::Number(a), Value::Number(b)) => {
            if a == b {
                stats.unchanged += 1;
                DiffNode {
                    status: DiffStatus::Unchanged,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            } else {
                stats.modified += 1;
                DiffNode {
                    status: DiffStatus::Modified,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            }
        }
        (Value::String(a), Value::String(b)) => {
            if a == b {
                stats.unchanged += 1;
                DiffNode {
                    status: DiffStatus::Unchanged,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            } else {
                stats.modified += 1;
                DiffNode {
                    status: DiffStatus::Modified,
                    key,
                    array_index,
                    left: Some(left.clone()),
                    right: Some(right.clone()),
                    children: Vec::new(),
                    depth,
                }
            }
        }

        // Type mismatch or any other combo → Modified
        _ => {
            stats.modified += 1;
            DiffNode {
                status: DiffStatus::Modified,
                key,
                array_index,
                left: Some(left.clone()),
                right: Some(right.clone()),
                children: Vec::new(),
                depth,
            }
        }
    }
}

/// Recursively mark an entire subtree as Removed.
fn removed_subtree(
    value: &Value,
    key: Option<String>,
    array_index: Option<usize>,
    depth: u16,
    stats: &mut DiffStats,
) -> DiffNode {
    match value {
        Value::Object(map) => {
            let children: Vec<DiffNode> = map
                .iter()
                .map(|(k, v)| removed_subtree(v, Some(k.clone()), None, depth + 1, stats))
                .collect();
            // Container itself doesn't count as a leaf removal; children do
            DiffNode {
                status: DiffStatus::Removed,
                key,
                array_index,
                left: Some(value.clone()),
                right: None,
                children,
                depth,
            }
        }
        Value::Array(arr) => {
            let children: Vec<DiffNode> = arr
                .iter()
                .enumerate()
                .map(|(i, v)| removed_subtree(v, None, Some(i), depth + 1, stats))
                .collect();
            DiffNode {
                status: DiffStatus::Removed,
                key,
                array_index,
                left: Some(value.clone()),
                right: None,
                children,
                depth,
            }
        }
        _ => {
            stats.removed += 1;
            DiffNode {
                status: DiffStatus::Removed,
                key,
                array_index,
                left: Some(value.clone()),
                right: None,
                children: Vec::new(),
                depth,
            }
        }
    }
}

/// Recursively mark an entire subtree as Added.
fn added_subtree(
    value: &Value,
    key: Option<String>,
    array_index: Option<usize>,
    depth: u16,
    stats: &mut DiffStats,
) -> DiffNode {
    match value {
        Value::Object(map) => {
            let children: Vec<DiffNode> = map
                .iter()
                .map(|(k, v)| added_subtree(v, Some(k.clone()), None, depth + 1, stats))
                .collect();
            DiffNode {
                status: DiffStatus::Added,
                key,
                array_index,
                left: None,
                right: Some(value.clone()),
                children,
                depth,
            }
        }
        Value::Array(arr) => {
            let children: Vec<DiffNode> = arr
                .iter()
                .enumerate()
                .map(|(i, v)| added_subtree(v, None, Some(i), depth + 1, stats))
                .collect();
            DiffNode {
                status: DiffStatus::Added,
                key,
                array_index,
                left: None,
                right: Some(value.clone()),
                children,
                depth,
            }
        }
        _ => {
            stats.added += 1;
            DiffNode {
                status: DiffStatus::Added,
                key,
                array_index,
                left: None,
                right: Some(value.clone()),
                children: Vec::new(),
                depth,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn diff_stats(left: Value, right: Value) -> (DiffStats, DiffStatus) {
        let result = diff(&left, &right);
        (result.stats, result.root.status)
    }

    #[test]
    fn identical_scalars() {
        let r = diff(&json!(42), &json!(42));
        assert_eq!(r.root.status, DiffStatus::Unchanged);
        assert_eq!(r.stats.unchanged, 1);
        assert_eq!(r.stats.modified, 0);
    }

    #[test]
    fn identical_documents() {
        let doc = json!({ "a": 1, "b": "hello", "c": [1, 2, 3] });
        let r = diff(&doc, &doc.clone());
        assert_eq!(r.root.status, DiffStatus::Unchanged);
        assert_eq!(r.stats.modified, 0);
        assert_eq!(r.stats.added, 0);
        assert_eq!(r.stats.removed, 0);
    }

    #[test]
    fn added_key() {
        let left = json!({ "a": 1 });
        let right = json!({ "a": 1, "b": 2 });
        let r = diff(&left, &right);
        assert_eq!(r.stats.added, 1);
        assert_eq!(r.stats.removed, 0);
        // Find child "b"
        let b = r.root.children.iter().find(|c| c.key.as_deref() == Some("b")).unwrap();
        assert_eq!(b.status, DiffStatus::Added);
    }

    #[test]
    fn removed_key() {
        let left = json!({ "a": 1, "b": 2 });
        let right = json!({ "a": 1 });
        let r = diff(&left, &right);
        assert_eq!(r.stats.removed, 1);
        let b = r.root.children.iter().find(|c| c.key.as_deref() == Some("b")).unwrap();
        assert_eq!(b.status, DiffStatus::Removed);
    }

    #[test]
    fn modified_key() {
        let left = json!({ "a": 1 });
        let right = json!({ "a": 2 });
        let r = diff(&left, &right);
        assert_eq!(r.stats.modified, 1);
        let a = r.root.children.iter().find(|c| c.key.as_deref() == Some("a")).unwrap();
        assert_eq!(a.status, DiffStatus::Modified);
    }

    #[test]
    fn nested_changes() {
        let left = json!({ "user": { "name": "Alice", "age": 30 } });
        let right = json!({ "user": { "name": "Alice", "age": 31 } });
        let r = diff(&left, &right);
        let user = r.root.children.iter().find(|c| c.key.as_deref() == Some("user")).unwrap();
        assert_eq!(user.status, DiffStatus::Modified);
        let age = user.children.iter().find(|c| c.key.as_deref() == Some("age")).unwrap();
        assert_eq!(age.status, DiffStatus::Modified);
        let name = user.children.iter().find(|c| c.key.as_deref() == Some("name")).unwrap();
        assert_eq!(name.status, DiffStatus::Unchanged);
    }

    #[test]
    fn type_change() {
        let left = json!({ "x": 42 });
        let right = json!({ "x": "42" });
        let r = diff(&left, &right);
        let x = r.root.children.iter().find(|c| c.key.as_deref() == Some("x")).unwrap();
        assert_eq!(x.status, DiffStatus::Modified);
        assert_eq!(r.stats.modified, 1);
    }

    #[test]
    fn array_length_mismatch_longer_right() {
        let left = json!([1, 2]);
        let right = json!([1, 2, 3]);
        let r = diff(&left, &right);
        assert_eq!(r.stats.added, 1);
        assert_eq!(r.stats.removed, 0);
        // index 2 should be Added
        let c2 = r.root.children.iter().find(|c| c.array_index == Some(2)).unwrap();
        assert_eq!(c2.status, DiffStatus::Added);
    }

    #[test]
    fn array_length_mismatch_longer_left() {
        let left = json!([1, 2, 3]);
        let right = json!([1, 2]);
        let r = diff(&left, &right);
        assert_eq!(r.stats.removed, 1);
        assert_eq!(r.stats.added, 0);
        let c2 = r.root.children.iter().find(|c| c.array_index == Some(2)).unwrap();
        assert_eq!(c2.status, DiffStatus::Removed);
    }

    #[test]
    fn null_equality() {
        let (stats, status) = diff_stats(json!(null), json!(null));
        assert_eq!(status, DiffStatus::Unchanged);
        assert_eq!(stats.unchanged, 1);
    }

    #[test]
    fn bool_modified() {
        let (stats, status) = diff_stats(json!(true), json!(false));
        assert_eq!(status, DiffStatus::Modified);
        assert_eq!(stats.modified, 1);
    }

    #[test]
    fn object_to_scalar_type_change() {
        let left = json!({ "a": 1 });
        let right = json!(42);
        let r = diff(&left, &right);
        assert_eq!(r.root.status, DiffStatus::Modified);
    }
}
