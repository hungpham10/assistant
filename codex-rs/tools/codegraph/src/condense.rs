//! Condensation utilities copied from codegraph-api/tools.rs

use serde::Serialize;
use serde_json::{Value, json};

const PATH_KEYS: [&str; 3] = ["file", "path", "matched_path"];

fn strip_root_prefix<'a>(path: &'a str, root: &str) -> &'a str {
    if let Some(rest) = path.strip_prefix(root) {
        if let Some(rest) = rest.strip_prefix('/') {
            return rest;
        }
    }
    path
}

pub fn relativize_paths(v: &mut Value, root: &str) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if PATH_KEYS.contains(&k.as_str()) {
                    if let Some(s) = val.as_str() {
                        *val = Value::String(strip_root_prefix(s, root).to_string());
                    }
                }
                relativize_paths(val, root);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                relativize_paths(item, root);
            }
        }
        _ => {}
    }
}

const ZERO_SENTINEL_KEYS: [&str; 3] = ["scope_id", "type_ref", "end_line"];

fn is_default_value(key: &str, v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        Value::Number(n) => ZERO_SENTINEL_KEYS.contains(&key) && n.as_f64() == Some(0.0),
    }
}

pub fn omit_defaults(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let old = std::mem::take(map);
            for (k, mut child) in old {
                omit_defaults(&mut child);
                if !is_default_value(&k, &child) {
                    map.insert(k, child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                omit_defaults(item);
            }
        }
        _ => {}
    }
}

pub fn emit_value(root: &str, mut v: Value) -> anyhow::Result<String> {
    relativize_paths(&mut v, root);
    Ok(serde_json::to_string(&v)?)
}

pub fn emit<T: Serialize>(root: &str, v: &T) -> anyhow::Result<String> {
    let value = serde_json::to_value(v)?;
    emit_value(root, value)
}
