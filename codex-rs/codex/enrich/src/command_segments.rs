//! Extracts ` ```@! ` enrichment blocks from user input.
//!
//! Only fenced code blocks whose info string is exactly `@!` (ignoring
//! surrounding whitespace) are considered enrichable. Everything else —
//! plain text, compiler diagnostics (`-->`), regular markdown fences — is
//! treated as prompt content and never parsed for commands.

/// Returns `(prompt, blocks)` where `prompt` is the input with all `@!`
/// fences removed and `blocks` contains the inner content of each fence in
/// source order.
///
/// If no `@!` fence is found, returns `(input.to_string(), vec![])` and the
/// caller should treat the original input as prompt-verbatim.
pub fn extract_enrich_blocks(input: &str) -> (String, Vec<String>) {
    let mut prompt = String::with_capacity(input.len());
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut fence_len: usize = 0;
    let mut block_buf = String::new();
    let mut first_prompt_segment = true;
    let mut found_any = false;

    for line in input.split_inclusive('\n') {
        let line_no_nl = line.strip_suffix('\n').unwrap_or(line);
        let line_no_crlf = line_no_nl.strip_suffix('\r').unwrap_or(line_no_nl);

        if in_block {
            if is_closing_fence(line_no_crlf, fence_len) {
                blocks.push(block_buf.clone());
                block_buf.clear();
                in_block = false;
                fence_len = 0;
                found_any = true;
            } else {
                block_buf.push_str(line);
            }
            continue;
        }

        if let Some((open_len, rest)) = parse_opening_fence(line_no_crlf) {
            // Check if this is a one-liner: content + closing fence on the same line.
            if let Some((content, _)) = split_one_liner(rest, open_len) {
                blocks.push(content.to_string());
                found_any = true;
                // fence itself is not added to prompt
            } else {
                in_block = true;
                fence_len = open_len;
                block_buf.clear();
                // If there's trailing content after `@!` on the opening line
                // (unusual but be permissive), treat it as start of block content.
                if !rest.trim().is_empty() {
                    block_buf.push_str(rest);
                    block_buf.push('\n');
                }
            }
            continue;
        }

        // Regular prompt line.
        if !first_prompt_segment || !prompt.is_empty() {
            prompt.push_str(line);
        } else {
            prompt.push_str(line);
        }
        first_prompt_segment = false;
    }

    // Handle final line without trailing newline - split_inclusive covers it,
    // but be defensive. Also handle unclosed fence (fail-open: keep as block).
    if in_block {
        blocks.push(block_buf);
        found_any = true;
    }

    if !found_any {
        return (input.to_string(), Vec::new());
    }

    (prompt, blocks)
}

/// Parses an opening ` ```@! ` fence. Returns `(fence_len, rest_after_info)`
/// where `rest_after_info` is the content after the `@!` marker on the same
/// line (for one-liner detection).
fn parse_opening_fence(line: &str) -> Option<(usize, &str)> {
    let trimmed_start = line.trim_start();
    let leading_ws_len = line.len() - trimmed_start.len();
    let backticks = trimmed_start
        .as_bytes()
        .iter()
        .take_while(|&&b| b == b'`')
        .count();
    if backticks < 3 {
        return None;
    }
    let after_ticks = &trimmed_start[backticks..];
    let after_ticks_trimmed = after_ticks.trim_start();
    if !after_ticks_trimmed.starts_with("@!") {
        return None;
    }
    let after_marker = &after_ticks_trimmed[2..];
    // `@!` must be the info string token - next char is whitespace, end, or
    // for one-liners, could be followed by content. Accept any trailing.
    let _ = leading_ws_len;
    Some((backticks, after_marker))
}

/// Attempts to split a one-liner fence where content and closing fence appear
/// after the `@!` marker on the same line.
/// Returns `(content, closing_fence)` if a closing fence of sufficient length
/// is found.
fn split_one_liner(rest: &str, open_len: usize) -> Option<(&str, &str)> {
    let rest = rest
        .strip_prefix(|c: char| c == ' ' || c == '\t')
        .unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    // Look for a closing fence (run of >= open_len backticks) at the end,
    // possibly preceded by space.
    let trimmed_end = rest.trim_end();
    let closing_start = trimmed_end
        .rfind(|c: char| c != '`')
        .map(|i| i + 1)
        .unwrap_or(0);
    let closing = &trimmed_end[closing_start..];
    if closing.len() >= open_len && !closing.is_empty() && closing.bytes().all(|b| b == b'`') {
        let content = trimmed_end[..closing_start].trim_end();
        if !content.is_empty() || closing_start > 0 {
            return Some((content, closing));
        }
    }
    None
}

fn is_closing_fence(line: &str, min_len: usize) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < min_len {
        return false;
    }
    trimmed.bytes().all(|b| b == b'`')
}
