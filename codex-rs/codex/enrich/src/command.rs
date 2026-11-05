use serde_json::Value as JsonValue;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct VerbCall {
    pub namespace: String,
    pub verb: String,
    pub args: JsonValue,
}

#[derive(Debug, Clone, Default)]
pub struct CommandPlan {
    pub commands: Vec<VerbCall>,
    pub prompt: String,
    pub raw: String,
}

#[derive(Debug, Error)]
pub enum RefError {
    #[error("unresolved reference: {0}")]
    Unresolved(String),
    #[error("out of range: {0}")]
    OutOfRange(usize),
}

pub trait CommandParser: Send + Sync {
    fn parse(&self, input: &str) -> CommandPlan;
}

#[derive(Debug, Default, Clone)]
pub struct AtCommandParser;

impl CommandParser for AtCommandParser {
    fn parse(&self, input: &str) -> CommandPlan {
        let raw = input.to_string();
        let mut commands = Vec::new();
        let mut remaining_prompt = input.to_string();

        if input.contains("->") {
            for part in input.split("->") {
                let part = part.trim();
                if let Some(cmd) = parse_single(part) {
                    commands.push(cmd);
                    if let Some(_paren_start) = part.find('(') {
                        if let Some(paren_end) = part.find(')') {
                            if let Some(start) = part.find('@') {
                                let to_remove = &part[start..=paren_end];
                                remaining_prompt = remaining_prompt.replacen(to_remove, "", 1);
                            }
                        }
                    }
                }
            }
        } else if let Some(cmd) = parse_single(input) {
            commands.push(cmd);
            if let Some(_paren_start) = input.find('(') {
                if let Some(paren_end) = input.find(')') {
                    if let Some(start) = input.find('@') {
                        let to_remove = &input[start..=paren_end];
                        remaining_prompt = remaining_prompt.replacen(to_remove, "", 1);
                    }
                }
            }
        }

        let mut prompt = remaining_prompt
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if prompt.trim().is_empty() && input.contains(':') {
            if let Some(colon_pos) = input.rfind(':') {
                prompt = input[colon_pos + 1..].trim().to_string();
            }
        } else if let Some(colon_pos) = prompt.find(':') {
            prompt = prompt[colon_pos + 1..].to_string();
        }

        CommandPlan {
            commands,
            prompt: prompt.trim().to_string(),
            raw,
        }
    }
}

fn parse_single(s: &str) -> Option<VerbCall> {
    if !s.contains('@') {
        return None;
    }
    if let Some(at_pos) = s.find('@') {
        let sub = &s[at_pos..];
        if let Some(paren_start) = sub.find('(') {
            if let Some(paren_end) = sub.find(')') {
                let prefix = &sub[1..paren_start];
                if let Some(dot_pos) = prefix.find('.') {
                    let namespace = prefix[..dot_pos].to_string();
                    let verb = prefix[dot_pos + 1..].to_string();
                    let args_str = &sub[paren_start + 1..paren_end];
                    let args: JsonValue = if args_str.trim().is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_str(&format!("{{{args_str}}}"))
                            .or_else(|_| serde_json::from_str(args_str))
                            .unwrap_or_else(|_| serde_json::json!({}))
                    };
                    return Some(VerbCall {
                        namespace,
                        verb,
                        args,
                    });
                }
            }
        }
    }
    None
}

pub fn resolve_refs(args: JsonValue, results: &[JsonValue]) -> Result<JsonValue, RefError> {
    resolve_refs_recursive(args, results)
}

fn resolve_refs_recursive(value: JsonValue, results: &[JsonValue]) -> Result<JsonValue, RefError> {
    match value {
        JsonValue::String(s) => {
            if let Some(path) = s.strip_prefix('$') {
                if path.is_empty() {
                    if results.is_empty() {
                        return Err(RefError::Unresolved(s));
                    }
                    return Ok(results[results.len() - 1].clone());
                }
                if let Ok(idx) = path.parse::<usize>() {
                    if idx >= results.len() {
                        return Err(RefError::OutOfRange(idx));
                    }
                    return Ok(results[idx].clone());
                }
                let path_to_use = if let Some(stripped) = path.strip_prefix('.') {
                    stripped
                } else {
                    path
                };
                let last_result = if results.is_empty() {
                    return Err(RefError::Unresolved(s));
                } else {
                    &results[results.len() - 1]
                };
                resolve_json_path(last_result, path_to_use).map_err(|_| RefError::Unresolved(s))
            } else {
                Ok(JsonValue::String(s))
            }
        }
        JsonValue::Array(arr) => {
            let mut resolved = Vec::new();
            for item in arr {
                resolved.push(resolve_refs_recursive(item, results)?);
            }
            Ok(JsonValue::Array(resolved))
        }
        JsonValue::Object(obj) => {
            let mut resolved = serde_json::Map::new();
            for (k, v) in obj {
                resolved.insert(k, resolve_refs_recursive(v, results)?);
            }
            Ok(JsonValue::Object(resolved))
        }
        other => Ok(other),
    }
}

fn resolve_json_path(value: &JsonValue, path: &str) -> Result<JsonValue, ()> {
    let mut current = value;
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut i = 0;
    let chars: Vec<char> = path.chars().collect();
    while i < chars.len() {
        if chars[i] == '.' {
            if !current_part.is_empty() {
                parts.push(current_part.clone());
                current_part.clear();
            }
        } else if chars[i] == '[' {
            if !current_part.is_empty() {
                parts.push(current_part.clone());
                current_part.clear();
            }
            i += 1;
            let mut idx_str = String::new();
            while i < chars.len() && chars[i] != ']' {
                idx_str.push(chars[i]);
                i += 1;
            }
            if let Ok(idx) = idx_str.parse::<usize>() {
                parts.push(format!("[{idx}]"));
            }
        } else {
            current_part.push(chars[i]);
        }
        i += 1;
    }
    if !current_part.is_empty() {
        parts.push(current_part);
    }

    for part in parts {
        if part.starts_with('[') && part.ends_with(']') {
            let idx_str = &part[1..part.len() - 1];
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let JsonValue::Array(arr) = current {
                    if idx >= arr.len() {
                        return Err(());
                    }
                    current = &arr[idx];
                } else {
                    return Err(());
                }
            } else {
                return Err(());
            }
        } else if let JsonValue::Object(obj) = current {
            if let Some(v) = obj.get(&part) {
                current = v;
            } else {
                return Err(());
            }
        } else {
            return Err(());
        }
    }
    Ok(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_at_command() {
        let parser = AtCommandParser;
        let plan = parser.parse("@codegraph.search(mode=lex): analyze this");
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].namespace, "codegraph");
        assert_eq!(plan.commands[0].verb, "search");
    }

    #[test]
    fn test_parse_chain() {
        let parser = AtCommandParser;
        let plan = parser.parse("@codegraph.search(query=test) -> @codegraph.flow(id=$.id)");
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[1].verb, "flow");
    }

    #[test]
    fn test_resolve_refs() {
        let results = vec![serde_json::json!({"symbols": [{"id": 42}]})];
        let args = serde_json::json!({"id": "$.symbols[0].id"});
        let resolved = resolve_refs(args, &results).unwrap();
        assert_eq!(resolved["id"], 42);
    }
}
