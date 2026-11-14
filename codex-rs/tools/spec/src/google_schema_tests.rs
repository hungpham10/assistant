use pretty_assertions::assert_eq;
use serde_json::json;

use super::strip_unsupported_keys_for_google;

#[test]
fn strips_unsupported_keys_at_every_depth() {
    let schema = json!({
        "type": "object",
        "properties": {
            "top": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "nested": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "leaf": { "type": "string", "default": "x" }
                            },
                            "required": ["leaf"]
                        }
                    },
                    "additionalProperties": { "type": "string" }
                },
                "additionalItems": { "type": "number" }
            }
        },
        "additionalProperties": false,
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://example.com/schema.json",
        "$comment": "internal",
        "required": ["top"]
    });

    assert_eq!(
        strip_unsupported_keys_for_google(schema),
        json!({
            "type": "object",
            "properties": {
                "top": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "nested": {
                                "type": "object",
                                "properties": {
                                    "leaf": { "type": "string" }
                                },
                                "required": ["leaf"]
                            }
                        }
                    }
                }
            },
            "required": ["top"]
        })
    );
}

#[test]
fn strips_inside_composition_keywords() {
    let schema = json!({
        "anyOf": [
            { "type": "string", "default": "a" },
            {
                "allOf": [
                    { "type": "object", "additionalProperties": false }
                ],
                "oneOf": [{ "type": "number" }],
                "prefixItems": [{ "type": "boolean", "examples": [true] }]
            }
        ]
    });

    assert_eq!(
        strip_unsupported_keys_for_google(schema),
        json!({
            "anyOf": [
                { "type": "string" },
                {
                    "allOf": [{ "type": "object" }],
                    "oneOf": [{ "type": "number" }],
                    "prefixItems": [{ "type": "boolean" }]
                }
            ]
        })
    );
}

#[test]
fn keeps_supported_keys() {
    let schema = json!({
        "type": "object",
        "description": "A tool",
        "properties": {
            "name": {
                "type": "string",
                "enum": ["a", "b"],
                "minLength": 1,
                "maxLength": 10,
                "pattern": "^[a-z]+$",
                "description": "Name"
            },
            "count": { "type": "integer", "minimum": 0, "maximum": 100, "format": "int32" },
            "either": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
            "nullable": { "type": "string", "nullable": true }
        },
        "required": ["name"]
    });

    assert_eq!(strip_unsupported_keys_for_google(schema.clone()), schema);
}

#[test]
fn handles_non_object_values() {
    assert_eq!(
        strip_unsupported_keys_for_google(json!([1, "two", true, null])),
        json!([1, "two", true, null])
    );
    assert_eq!(
        strip_unsupported_keys_for_google(json!("plain")),
        json!("plain")
    );
    assert_eq!(strip_unsupported_keys_for_google(json!(null)), json!(null));
}
