//! Tokenization utilities for codegraph queries

pub fn tokenize(question: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in question.split_whitespace() {
        // Split by common separators
        let mut current = String::new();
        for ch in token.chars() {
            if ch == '_' || ch == '-' || ch == '/' || ch == '.' {
                if !current.is_empty() {
                    tokens.push(current.to_lowercase());
                    current.clear();
                }
            } else if ch.is_uppercase()
                && !current.is_empty()
                && !matches!(current.chars().last(), Some(c) if c.is_uppercase())
            {
                tokens.push(current.to_lowercase());
                current.clear();
                current.push(ch);
            } else {
                current.push(ch);
            }
        }
        if !current.is_empty() {
            tokens.push(current.to_lowercase());
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("OrderService getUser");
        assert!(tokens.contains(&"orderservice".to_string()));
    }
}
