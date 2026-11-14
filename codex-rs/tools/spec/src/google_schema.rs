//! Sanitize tool JSON schemas for the Google Gemini `generateContent` API.
//!
//! Gemini only accepts a subset of the OpenAPI 3.0 schema vocabulary in
//! `function_declarations[*].parameters`. Fields such as
//! `additionalProperties` are rejected with a 400 `INVALID_ARGUMENT` error,
//! including when they appear in nested `properties` or `items`. Tool schemas
//! built elsewhere (MCP tools, built-in tools) routinely contain
//! `additionalProperties: false`, so the schema must be stripped before it is
//! sent to the Google wire API.

/// JSON Schema keywords Gemini does not support, stripped recursively from
/// serialized tool parameter schemas.
const UNSUPPORTED_KEYS: &[&str] = &[
    "additionalProperties",
    "additionalItems",
    "$schema",
    "$id",
    "$anchor",
    "$comment",
    "default",
    "examples",
];

/// Strip schema keywords that the Google Gemini API rejects, recursively
/// through `properties`, `items`, and the composition keywords (`anyOf`,
/// `oneOf`, `allOf`, `prefixItems`).
pub fn strip_unsupported_keys_for_google(mut value: serde_json::Value) -> serde_json::Value {
    strip_unsupported_keys_in_place(&mut value);
    value
}

fn strip_unsupported_keys_in_place(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for key in UNSUPPORTED_KEYS {
                map.remove(*key);
            }
            for (_, child) in map.iter_mut() {
                strip_unsupported_keys_in_place(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_unsupported_keys_in_place(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "google_schema_tests.rs"]
mod tests;
