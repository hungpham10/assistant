use serde_json::Value as JsonValue;

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
}

pub trait CommandParser: Send + Sync {
    fn parse(&self, input: &str) -> CommandPlan;
}

#[derive(Debug, Default, Clone)]
pub struct AtCommandParser;

impl CommandParser for AtCommandParser {
    fn parse(&self, input: &str) -> CommandPlan {
        let mut commands = Vec::new();
        let mut prompt = input.to_string();
        // Simple parser: look for @namespace.verb(...) patterns
        // This is a basic implementation - can be enhanced
        let mut i = 0;
        let bytes = input.as_bytes();
        let len = bytes.len();
        while i < len {
            if bytes[i] == b'@' {
                // Try to parse @ns.verb
                if let Some((cmd, consumed)) = parse_at_command(&input[i..]) {
                    commands.push(cmd);
                    // Remove the consumed part from prompt
                    prompt = prompt.replacen(&input[i..i + consumed], "", 1);
                    i += consumed;
                    continue;
                }
            }
            i += 1;
        }
        // Clean up extra whitespace
        prompt = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        CommandPlan { commands, prompt }
    }
}

fn parse_at_command(s: &str) -> Option<(VerbCall, usize)> {
    // Very basic parser - will be improved
    // Look for pattern like @codegraph.search(...)
    if !s.starts_with('@') {
        return None;
    }
    // Find the end of parens
    if let Some(paren_start) = s.find('(') {
        if let Some(paren_end) = s.find(')') {
            let prefix = &s[1..paren_start]; // without @
            if let Some(dot_pos) = prefix.find('.') {
                let namespace = prefix[..dot_pos].to_string();
                let verb = prefix[dot_pos + 1..].to_string();
                let args_str = &s[paren_start + 1..paren_end];
                // Try to parse as JSON, if fails use empty object
                let args: JsonValue = serde_json::from_str(&format!("{{{args_str}}}"))
                    .or_else(|_| serde_json::from_str(args_str))
                    .unwrap_or_else(|_| serde_json::json!({}));
                return Some((
                    VerbCall {
                        namespace,
                        verb,
                        args,
                    },
                    paren_end + 1,
                ));
            }
        }
    }
    None
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
}
