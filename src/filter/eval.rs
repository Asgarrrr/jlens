use serde_json::Value;

use super::FilterError;
use super::parse::{ArithOp, BuiltinFn, CmpOp, Expr};

/// Apply `expr` on a single input value and return all output values.
pub fn apply(input: &Value, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    apply_stream(vec![input.clone()], expr)
}

fn apply_stream(inputs: Vec<Value>, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    let mut results = Vec::new();
    for input in inputs {
        results.extend(apply_one(input, expr)?);
    }
    Ok(results)
}

fn apply_one(input: Value, expr: &Expr) -> Result<Vec<Value>, FilterError> {
    match expr {
        Expr::Identity => Ok(vec![input]),

        Expr::Field(name) => match input {
            Value::Object(ref map) => Ok(map
                .get(name.as_str())
                .cloned()
                .into_iter()
                .collect()),
            _ => Ok(vec![]),
        },

        Expr::Index(n) => match input {
            Value::Array(ref arr) => {
                let len = arr.len() as i64;
                let idx = if *n < 0 { len + n } else { *n };
                if idx >= 0 && idx < len {
                    Ok(vec![arr[idx as usize].clone()])
                } else {
                    Ok(vec![])
                }
            }
            _ => Ok(vec![]),
        },

        Expr::Slice(lo, hi) => match input {
            Value::Array(ref arr) => {
                let len = arr.len() as i64;
                let start = resolve_bound(*lo, len, 0).max(0).min(len) as usize;
                let end = resolve_bound(*hi, len, len).max(0).min(len) as usize;
                let end = end.max(start);
                Ok(vec![Value::Array(arr[start..end].to_vec())])
            }
            _ => Ok(vec![]),
        },

        Expr::Iterate => match input {
            Value::Array(arr) => Ok(arr),
            Value::Object(map) => Ok(map.into_values().collect()),
            _ => Ok(vec![]),
        },

        Expr::Pipe(a, b) => apply_stream(apply_one(input, a)?, b),
        Expr::Chain(a, b) => apply_stream(apply_one(input, a)?, b),
        Expr::Paren(inner) => apply_one(input, inner),

        // Literals
        Expr::StringLit(s) => Ok(vec![Value::String(s.clone())]),
        Expr::NumberLit(f) => Ok(vec![serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null)]),
        Expr::BoolLit(b) => Ok(vec![Value::Bool(*b)]),
        Expr::NullLit => Ok(vec![Value::Null]),

        // Comparison
        Expr::Compare(a, op, b) => {
            let left = apply_one(input.clone(), a)?.into_iter().next().unwrap_or(Value::Null);
            let right = apply_one(input, b)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![Value::Bool(compare_values(&left, op, &right))])
        }

        // Boolean logic
        Expr::And(a, b) => {
            let left = apply_one(input.clone(), a)?.into_iter().next().unwrap_or(Value::Null);
            if !is_truthy(&left) {
                Ok(vec![Value::Bool(false)])
            } else {
                let right = apply_one(input, b)?.into_iter().next().unwrap_or(Value::Null);
                Ok(vec![Value::Bool(is_truthy(&right))])
            }
        }
        Expr::Or(a, b) => {
            let left = apply_one(input.clone(), a)?.into_iter().next().unwrap_or(Value::Null);
            if is_truthy(&left) {
                Ok(vec![Value::Bool(true)])
            } else {
                let right = apply_one(input, b)?.into_iter().next().unwrap_or(Value::Null);
                Ok(vec![Value::Bool(is_truthy(&right))])
            }
        }
        Expr::Not(inner) => {
            let val = apply_one(input, inner)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![Value::Bool(!is_truthy(&val))])
        }

        // Arithmetic
        Expr::Arith(a, op, b) => {
            let left = apply_one(input.clone(), a)?.into_iter().next().unwrap_or(Value::Null);
            let right = apply_one(input, b)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![arith(&left, op, &right)])
        }

        // Select — keep input if predicate is truthy, else drop
        Expr::Select(pred) => {
            let result = apply_one(input.clone(), pred)?
                .into_iter()
                .next()
                .unwrap_or(Value::Null);
            if is_truthy(&result) {
                Ok(vec![input])
            } else {
                Ok(vec![])
            }
        }

        // Map — transform each element of an array
        Expr::Map(body) => match input {
            Value::Array(arr) => {
                let mut out = Vec::new();
                for item in arr {
                    if let Some(v) = apply_one(item, body)?.into_iter().next() {
                        out.push(v);
                    }
                }
                Ok(vec![Value::Array(out)])
            }
            _ => Ok(vec![]),
        },

        // SortBy — sort array elements by a key expression
        Expr::SortBy(key_expr) => match input {
            Value::Array(mut arr) => {
                let mut keyed: Vec<(Value, Value)> = arr
                    .drain(..)
                    .map(|item| {
                        let key = apply_one(item.clone(), key_expr)
                            .ok()
                            .and_then(|mut v| v.pop())
                            .unwrap_or(Value::Null);
                        (key, item)
                    })
                    .collect();
                keyed.sort_by(|(a, _), (b, _)| cmp_values(a, b));
                Ok(vec![Value::Array(keyed.into_iter().map(|(_, v)| v).collect())])
            }
            _ => Ok(vec![]),
        },

        Expr::Builtin(b) => apply_builtin(input, b),
    }
}

fn apply_builtin(input: Value, builtin: &BuiltinFn) -> Result<Vec<Value>, FilterError> {
    match builtin {
        BuiltinFn::Length => {
            let len = match &input {
                Value::Null => 0,
                Value::String(s) => s.chars().count(),
                Value::Array(arr) => arr.len(),
                Value::Object(map) => map.len(),
                _ => return Ok(vec![]),
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
                Ok(vec![Value::Array((0..arr.len()).map(|i| Value::Number(i.into())).collect())])
            }
            _ => Ok(vec![]),
        },

        BuiltinFn::Values => match input {
            Value::Object(map) => Ok(vec![Value::Array(map.into_values().collect())]),
            Value::Array(arr) => Ok(vec![Value::Array(arr)]),
            _ => Ok(vec![]),
        },

        BuiltinFn::Type => {
            let t = match &input {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            Ok(vec![Value::String(t.into())])
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

        BuiltinFn::First => match input {
            Value::Array(arr) => Ok(arr.into_iter().next().into_iter().collect()),
            _ => Ok(vec![]),
        },

        BuiltinFn::Last => match input {
            Value::Array(arr) => Ok(arr.into_iter().last().into_iter().collect()),
            _ => Ok(vec![]),
        },

        BuiltinFn::Reverse => match input {
            Value::Array(mut arr) => { arr.reverse(); Ok(vec![Value::Array(arr)]) }
            _ => Ok(vec![]),
        },

        BuiltinFn::Unique => match input {
            Value::Array(arr) => {
                let mut seen = Vec::new();
                for item in arr {
                    if !seen.contains(&item) { seen.push(item); }
                }
                Ok(vec![Value::Array(seen)])
            }
            _ => Ok(vec![]),
        },

        BuiltinFn::Sort => match input {
            Value::Array(mut arr) => { arr.sort_by(cmp_values); Ok(vec![Value::Array(arr)]) }
            _ => Ok(vec![]),
        },

        BuiltinFn::Min => match input {
            Value::Array(arr) => Ok(arr.into_iter().min_by(cmp_values).into_iter().collect()),
            _ => Ok(vec![]),
        },

        BuiltinFn::Max => match input {
            Value::Array(arr) => Ok(arr.into_iter().max_by(cmp_values).into_iter().collect()),
            _ => Ok(vec![]),
        },

        BuiltinFn::Not => Ok(vec![Value::Bool(!is_truthy(&input))]),

        BuiltinFn::ToNumber => match &input {
            Value::Number(_) => Ok(vec![input]),
            Value::String(s) => Ok(vec![s.parse::<f64>().ok()
                .and_then(|f| serde_json::Number::from_f64(f).map(Value::Number))
                .unwrap_or(Value::Null)]),
            _ => Ok(vec![Value::Null]),
        },

        BuiltinFn::ToString => {
            let s = match &input {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            Ok(vec![Value::String(s)])
        }

        BuiltinFn::AsciiDowncase => match input {
            Value::String(s) => Ok(vec![Value::String(s.to_ascii_lowercase())]),
            _ => Ok(vec![input]),
        },
    }
}

fn resolve_bound(bound: Option<i64>, len: i64, default: i64) -> i64 {
    match bound {
        None => default,
        Some(n) if n < 0 => (len + n).max(0),
        Some(n) => n,
    }
}

fn is_truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

fn compare_values(left: &Value, op: &CmpOp, right: &Value) -> bool {
    match op {
        CmpOp::Eq => cmp_values(left, right) == std::cmp::Ordering::Equal,
        CmpOp::Ne => cmp_values(left, right) != std::cmp::Ordering::Equal,
        CmpOp::Lt => cmp_values(left, right) == std::cmp::Ordering::Less,
        CmpOp::Le => !matches!(cmp_values(left, right), std::cmp::Ordering::Greater),
        CmpOp::Gt => cmp_values(left, right) == std::cmp::Ordering::Greater,
        CmpOp::Ge => !matches!(cmp_values(left, right), std::cmp::Ordering::Less),
    }
}

fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            a.as_f64().unwrap_or(0.0).partial_cmp(&b.as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        _ => type_rank(a).cmp(&type_rank(b)),
    }
}

fn type_rank(v: &Value) -> u8 {
    match v { Value::Null => 0, Value::Bool(_) => 1, Value::Number(_) => 2, Value::String(_) => 3, Value::Array(_) => 4, Value::Object(_) => 5 }
}

fn arith(left: &Value, op: &ArithOp, right: &Value) -> Value {
    let a = left.as_f64().unwrap_or(0.0);
    let b = right.as_f64().unwrap_or(0.0);
    let result = match op {
        ArithOp::Add => a + b,
        ArithOp::Sub => a - b,
        ArithOp::Mul => a * b,
        ArithOp::Div => if b == 0.0 { return Value::Null } else { a / b },
    };
    serde_json::Number::from_f64(result).map(Value::Number).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parse::parse;
    use serde_json::json;

    fn run(input: &Value, expr: &str) -> Vec<Value> {
        let e = parse(expr).unwrap_or_else(|e| panic!("parse error for {expr:?}: {e}"));
        apply(input, &e).unwrap_or_else(|e| panic!("apply error for {expr:?}: {e}"))
    }

    fn obj() -> Value {
        json!({"name": "Alice", "age": 30, "scores": [10, 20, 30], "address": {"city": "Wonderland", "zip": "12345"}})
    }

    #[test] fn identity_returns_input() { assert_eq!(run(&json!(42), "."), vec![json!(42)]); }
    #[test] fn field_access() { assert_eq!(run(&obj(), ".name"), vec![json!("Alice")]); }
    #[test] fn missing_field_is_empty() { assert_eq!(run(&obj(), ".nonexistent"), vec![] as Vec<Value>); }
    #[test] fn chained_fields() { assert_eq!(run(&obj(), ".address.city"), vec![json!("Wonderland")]); }
    #[test] fn index_access() { assert_eq!(run(&obj(), ".scores[0]"), vec![json!(10)]); }
    #[test] fn negative_index() { assert_eq!(run(&obj(), ".scores[-1]"), vec![json!(30)]); }
    #[test] fn out_of_bounds_index_is_empty() { assert_eq!(run(&obj(), ".scores[99]"), vec![] as Vec<Value>); }
    #[test] fn array_slice() { assert_eq!(run(&obj(), ".scores[0:2]"), vec![json!([10, 20])]); }
    #[test] fn iterate_array() { assert_eq!(run(&json!([1,2,3]), ".[]"), vec![json!(1), json!(2), json!(3)]); }
    #[test] fn field_then_iterate() { assert_eq!(run(&obj(), ".scores[]"), vec![json!(10), json!(20), json!(30)]); }
    #[test] fn pipe_to_length_array() { assert_eq!(run(&obj(), ".scores | length"), vec![json!(3)]); }
    #[test] fn pipe_to_length_string() { assert_eq!(run(&obj(), ".name | length"), vec![json!(5)]); }
    #[test] fn pipe_to_length_null() { assert_eq!(run(&json!(null), ". | length"), vec![json!(0)]); }
    #[test] fn pipe_to_keys() { assert_eq!(run(&json!({"b": 2, "a": 1}), ". | keys"), vec![json!(["a", "b"])]); }
    #[test] fn keys_on_array() { assert_eq!(run(&json!([10,20,30]), ". | keys"), vec![json!([0, 1, 2])]); }
    #[test] fn pipe_to_type() { assert_eq!(run(&json!(null), ". | type"), vec![json!("null")]); }
    #[test] fn flatten_one_level() { assert_eq!(run(&json!([[1,2],[3,[4,5]]]), ". | flatten"), vec![json!([1, 2, 3, [4, 5]])]); }
    #[test] fn flatten_flat_array() { assert_eq!(run(&json!([1,2,3]), ". | flatten"), vec![json!([1, 2, 3])]); }
    #[test] fn identity_with_object() { let v = obj(); assert_eq!(run(&v, "."), vec![v]); }
    #[test] fn index_on_non_array() { assert_eq!(run(&json!("hello"), ".[0]"), vec![] as Vec<Value>); }
    #[test] fn field_on_non_object() { assert_eq!(run(&json!(42), ".foo"), vec![] as Vec<Value>); }

    // New features
    #[test] fn select_filter() { assert_eq!(run(&json!([1,2,3,4,5]), ".[] | select(. > 3)"), vec![json!(4), json!(5)]); }
    #[test] fn select_with_field() { assert_eq!(run(&json!([{"age":25},{"age":35}]), ".[] | select(.age >= 30)"), vec![json!({"age":35})]); }
    #[test] fn comparison_eq() { assert_eq!(run(&json!({"a":1}), ".a == 1"), vec![json!(true)]); }
    #[test] fn comparison_ne() { assert_eq!(run(&json!({"a":2}), ".a != 1"), vec![json!(true)]); }
    #[test] fn comparison_gt() { assert_eq!(run(&json!({"a":3,"b":1}), ".a > .b"), vec![json!(true)]); }
    #[test] fn boolean_and() { assert_eq!(run(&json!({"a":3}), ".a > 1 and .a < 5"), vec![json!(true)]); }
    #[test] fn boolean_or() { assert_eq!(run(&json!({"a":1,"b":20}), ".a > 10 or .b > 10"), vec![json!(true)]); }
    #[test] fn boolean_not() { assert_eq!(run(&json!(true), "not ."), vec![json!(false)]); }
    #[test] fn arithmetic_mul() { assert_eq!(run(&json!({"price":10,"qty":3}), ".price * .qty"), vec![json!(30.0)]); }
    #[test] fn arithmetic_add() { assert_eq!(run(&json!({"a":1,"b":2}), ".a + .b"), vec![json!(3.0)]); }
    #[test] fn arithmetic_div_zero() { assert_eq!(run(&json!({"a":1}), ".a / 0"), vec![json!(null)]); }
    #[test] fn map_expr() { assert_eq!(run(&json!([{"name":"a"},{"name":"b"}]), "map(.name)"), vec![json!(["a","b"])]); }
    #[test] fn sort_by_expr() { assert_eq!(run(&json!([{"age":30},{"age":20}]), "sort_by(.age)"), vec![json!([{"age":20},{"age":30}])]); }
    #[test] fn builtin_first() { assert_eq!(run(&json!([1,2,3]), ". | first"), vec![json!(1)]); }
    #[test] fn builtin_last() { assert_eq!(run(&json!([1,2,3]), ". | last"), vec![json!(3)]); }
    #[test] fn builtin_reverse() { assert_eq!(run(&json!([1,2,3]), ". | reverse"), vec![json!([3,2,1])]); }
    #[test] fn builtin_unique() { assert_eq!(run(&json!([1,2,1,3]), ". | unique"), vec![json!([1,2,3])]); }
    #[test] fn builtin_sort() { assert_eq!(run(&json!([3,1,2]), ". | sort"), vec![json!([1,2,3])]); }
    #[test] fn builtin_min() { assert_eq!(run(&json!([3,1,2]), ". | min"), vec![json!(1)]); }
    #[test] fn builtin_max() { assert_eq!(run(&json!([3,1,2]), ". | max"), vec![json!(3)]); }
    #[test] fn builtin_ascii_downcase() { assert_eq!(run(&json!("HELLO"), ". | ascii_downcase"), vec![json!("hello")]); }
    #[test] fn builtin_to_string() { assert_eq!(run(&json!(42), ". | to_string"), vec![json!("42")]); }
    #[test] fn builtin_to_number() { assert_eq!(run(&json!("42"), ". | to_number"), vec![json!(42.0)]); }
    #[test] fn string_equality() { assert_eq!(run(&json!({"name":"Alice"}), ".name == \"Alice\""), vec![json!(true)]); }
    #[test] fn complex_pipeline() {
        let data = json!([{"name":"Alice","age":30},{"name":"Bob","age":25},{"name":"Charlie","age":35}]);
        assert_eq!(run(&data, ".[] | select(.age >= 30) | .name"), vec![json!("Alice"), json!("Charlie")]);
    }
    #[test] fn parenthesized_arithmetic() { assert_eq!(run(&json!({"a":2,"b":3}), "(.a + .b) * 2"), vec![json!(10.0)]); }
}
