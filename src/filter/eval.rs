use serde_json::Value;

use super::FilterError;
use super::parse::{BuiltinFn, Expr};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Evaluate `expr` on a single input value and return all output values.
pub fn eval(input: &Value, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    eval_stream(vec![input.clone()], expr)
}

// ---------------------------------------------------------------------------
// Stream evaluator
// ---------------------------------------------------------------------------

/// Apply `expr` to every value in `inputs`, collecting all outputs into a flat
/// Vec (the standard "stream" semantics used by jq).
fn eval_stream(inputs: Vec<Value>, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    let mut results = Vec::new();

    for input in inputs {
        let produced = eval_one(input, expr)?;
        results.extend(produced);
    }

    Ok(results)
}

/// Evaluate `expr` against a single input value, returning zero or more
/// output values.
fn eval_one(input: Value, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    match expr {
        // ----------------------------------------------------------------
        // Identity — pass through unchanged
        // ----------------------------------------------------------------
        Expr::Identity => Ok(vec![input]),

        // ----------------------------------------------------------------
        // Field access — `.foo`
        // ----------------------------------------------------------------
        Expr::Field(name) => match input {
            Value::Object(ref map) => match map.get(name.as_str()) {
                Some(v) => Ok(vec![v.clone()]),
                None => Ok(vec![]),
            },
            // Field access on a non-object silently produces nothing
            _ => Ok(vec![]),
        },

        // ----------------------------------------------------------------
        // Array index — `.[n]`
        // ----------------------------------------------------------------
        Expr::Index(n) => match input {
            Value::Array(ref arr) => {
                let len = arr.len() as i64;
                let idx = if *n < 0 { len + n } else { *n };
                if idx < 0 || idx >= len {
                    Ok(vec![])
                } else {
                    Ok(vec![arr[idx as usize].clone()])
                }
            }
            _ => Ok(vec![]),
        },

        // ----------------------------------------------------------------
        // Array slice — `.[n:m]`
        // ----------------------------------------------------------------
        Expr::Slice(lo, hi) => match input {
            Value::Array(ref arr) => {
                let len = arr.len() as i64;
                let start = resolve_bound(*lo, len, 0);
                let end = resolve_bound(*hi, len, len);
                let start = start.max(0).min(len) as usize;
                let end = end.max(0).min(len) as usize;
                let end = end.max(start);
                Ok(vec![Value::Array(arr[start..end].to_vec())])
            }
            _ => Ok(vec![]),
        },

        // ----------------------------------------------------------------
        // Iterate — `.[]`
        // ----------------------------------------------------------------
        Expr::Iterate => match input {
            Value::Array(arr) => Ok(arr),
            Value::Object(map) => Ok(map.into_values().collect()),
            _ => Ok(vec![]),
        },

        // ----------------------------------------------------------------
        // Pipe — `a | b`
        // ----------------------------------------------------------------
        Expr::Pipe(a, b) => {
            let mid = eval_one(input, a)?;
            eval_stream(mid, b)
        }

        // ----------------------------------------------------------------
        // Chain — `.foo.bar` (semantically identical to pipe)
        // ----------------------------------------------------------------
        Expr::Chain(a, b) => {
            let mid = eval_one(input, a)?;
            eval_stream(mid, b)
        }

        // ----------------------------------------------------------------
        // Builtins
        // ----------------------------------------------------------------
        Expr::Builtin(builtin) => eval_builtin(input, builtin),
    }
}

fn eval_builtin(input: Value, builtin: &BuiltinFn) -> Result<Vec<Value>, FilterError> {
    match builtin {
        BuiltinFn::Length => {
            let len = match &input {
                Value::Null => 0,
                Value::String(s) => s.chars().count(),
                Value::Array(arr) => arr.len(),
                Value::Object(map) => map.len(),
                Value::Bool(_) | Value::Number(_) => {
                    // jq errors on number/bool for `length`; we return 0 to stay
                    // non-fatal per the spec ("else → empty" style).
                    return Ok(vec![]);
                }
            };
            Ok(vec![Value::Number(len.into())])
        }

        BuiltinFn::Keys => match input {
            Value::Object(map) => {
                let mut keys: Vec<Value> = map.keys().map(|k| Value::String(k.clone())).collect();
                keys.sort_by(|a, b| a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")));
                Ok(vec![Value::Array(keys)])
            }
            Value::Array(arr) => {
                // For arrays, keys = indices
                let keys: Vec<Value> = (0..arr.len()).map(|i| Value::Number(i.into())).collect();
                Ok(vec![Value::Array(keys)])
            }
            _ => Ok(vec![]),
        },

        BuiltinFn::Values => match input {
            Value::Object(map) => {
                let vals: Vec<Value> = map.into_values().collect();
                Ok(vec![Value::Array(vals)])
            }
            Value::Array(arr) => Ok(vec![Value::Array(arr)]),
            _ => Ok(vec![]),
        },

        BuiltinFn::Type => {
            let type_name = match &input {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            Ok(vec![Value::String(type_name.to_string())])
        }

        BuiltinFn::Flatten => match input {
            Value::Array(arr) => {
                let mut out = Vec::new();
                for item in arr {
                    match item {
                        Value::Array(inner) => out.extend(inner),
                        other => out.push(other),
                    }
                }
                Ok(vec![Value::Array(out)])
            }
            _ => Ok(vec![]),
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve an optional slice bound, applying negative-index semantics.
/// `default` is used when the bound is absent.
fn resolve_bound(bound: Option<i64>, len: i64, default: i64) -> i64 {
    match bound {
        None => default,
        Some(n) if n < 0 => (len + n).max(0),
        Some(n) => n,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parse::parse;
    use serde_json::json;

    fn run(input: &Value, expr: &str) -> Vec<Value> {
        let e = parse(expr).unwrap_or_else(|e| panic!("parse error for {:?}: {}", expr, e));
        eval(input, &e).unwrap_or_else(|e| panic!("eval error for {:?}: {}", expr, e))
    }

    fn obj() -> Value {
        json!({
            "name": "Alice",
            "age": 30,
            "scores": [10, 20, 30],
            "address": {
                "city": "Wonderland",
                "zip": "12345"
            }
        })
    }

    #[test]
    fn identity_returns_input() {
        let v = json!(42);
        assert_eq!(run(&v, "."), vec![json!(42)]);
    }

    #[test]
    fn field_access() {
        assert_eq!(run(&obj(), ".name"), vec![json!("Alice")]);
        assert_eq!(run(&obj(), ".age"), vec![json!(30)]);
    }

    #[test]
    fn missing_field_is_empty() {
        assert_eq!(run(&obj(), ".nonexistent"), vec![] as Vec<Value>);
    }

    #[test]
    fn chained_fields() {
        assert_eq!(run(&obj(), ".address.city"), vec![json!("Wonderland")]);
    }

    #[test]
    fn index_access() {
        assert_eq!(run(&obj(), ".scores[0]"), vec![json!(10)]);
        assert_eq!(run(&obj(), ".scores[2]"), vec![json!(30)]);
    }

    #[test]
    fn negative_index() {
        assert_eq!(run(&obj(), ".scores[-1]"), vec![json!(30)]);
        assert_eq!(run(&obj(), ".scores[-2]"), vec![json!(20)]);
    }

    #[test]
    fn out_of_bounds_index_is_empty() {
        assert_eq!(run(&obj(), ".scores[99]"), vec![] as Vec<Value>);
    }

    #[test]
    fn array_slice() {
        assert_eq!(run(&obj(), ".scores[0:2]"), vec![json!([10, 20])]);
        assert_eq!(run(&obj(), ".scores[1:]"), vec![json!([20, 30])]);
        assert_eq!(run(&obj(), ".scores[:2]"), vec![json!([10, 20])]);
    }

    #[test]
    fn iterate_array() {
        let v = json!([1, 2, 3]);
        assert_eq!(run(&v, ".[]"), vec![json!(1), json!(2), json!(3)]);
    }

    #[test]
    fn iterate_object() {
        let v = json!({"a": 1, "b": 2});
        let mut result = run(&v, ".[]");
        result.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        assert!(result.contains(&json!(1)));
        assert!(result.contains(&json!(2)));
    }

    #[test]
    fn field_then_iterate() {
        let result = run(&obj(), ".scores[]");
        assert_eq!(result, vec![json!(10), json!(20), json!(30)]);
    }

    #[test]
    fn pipe_to_length_array() {
        assert_eq!(run(&obj(), ".scores | length"), vec![json!(3)]);
    }

    #[test]
    fn pipe_to_length_string() {
        assert_eq!(run(&obj(), ".name | length"), vec![json!(5)]);
    }

    #[test]
    fn pipe_to_length_null() {
        let v = json!(null);
        assert_eq!(run(&v, ". | length"), vec![json!(0)]);
    }

    #[test]
    fn pipe_to_keys() {
        let v = json!({"b": 2, "a": 1});
        assert_eq!(run(&v, ". | keys"), vec![json!(["a", "b"])]);
    }

    #[test]
    fn keys_on_array() {
        let v = json!([10, 20, 30]);
        assert_eq!(run(&v, ". | keys"), vec![json!([0, 1, 2])]);
    }

    #[test]
    fn pipe_to_values() {
        let v = json!({"a": 1, "b": 2});
        let result = run(&v, ". | values");
        assert_eq!(result.len(), 1);
        if let Value::Array(arr) = &result[0] {
            let mut nums: Vec<i64> = arr.iter().map(|v| v.as_i64().unwrap()).collect();
            nums.sort();
            assert_eq!(nums, vec![1, 2]);
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn pipe_to_type() {
        assert_eq!(run(&json!(null), ". | type"), vec![json!("null")]);
        assert_eq!(run(&json!(true), ". | type"), vec![json!("boolean")]);
        assert_eq!(run(&json!(42), ". | type"), vec![json!("number")]);
        assert_eq!(run(&json!("hi"), ". | type"), vec![json!("string")]);
        assert_eq!(run(&json!([]), ". | type"), vec![json!("array")]);
        assert_eq!(run(&json!({}), ". | type"), vec![json!("object")]);
    }

    #[test]
    fn flatten_one_level() {
        let v = json!([[1, 2], [3, [4, 5]]]);
        assert_eq!(run(&v, ". | flatten"), vec![json!([1, 2, 3, [4, 5]])]);
    }

    #[test]
    fn flatten_flat_array() {
        let v = json!([1, 2, 3]);
        assert_eq!(run(&v, ". | flatten"), vec![json!([1, 2, 3])]);
    }

    #[test]
    fn identity_with_object() {
        let v = obj();
        assert_eq!(run(&v, "."), vec![v]);
    }

    #[test]
    fn index_on_non_array_is_empty() {
        assert_eq!(run(&json!("hello"), ".[0]"), vec![] as Vec<Value>);
    }

    #[test]
    fn field_on_non_object_is_empty() {
        assert_eq!(run(&json!(42), ".foo"), vec![] as Vec<Value>);
    }
}
