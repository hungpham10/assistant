use super::enforce_token_budget;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

fn enrichment_item(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn item_text(item: &ResponseItem) -> String {
    match item {
        ResponseItem::Message { content, .. } => content
            .iter()
            .map(|c| match c {
                ContentItem::InputText { text } => text.clone(),
                _ => String::new(),
            })
            .collect(),
        _ => String::new(),
    }
}

#[test]
fn budget_keeps_small_items_intact() {
    let mut items = vec![enrichment_item("{\"echo\":\"hi\"}")];
    enforce_token_budget(&mut items, 1000);
    assert_eq!(item_text(&items[0]), "{\"echo\":\"hi\"}");
}

#[test]
fn budget_truncates_overflowing_item_in_place() {
    let long = "x".repeat(10_000);
    let mut items = vec![enrichment_item(&long)];
    enforce_token_budget(&mut items, 64);
    let kept = item_text(&items[0]);
    assert!(kept.len() < long.len(), "item should be truncated");
    assert!(!kept.is_empty(), "partial budget should still keep content");
}

#[test]
fn budget_zero_empties_all_items() {
    let mut items = vec![
        enrichment_item("{\"echo\":\"a\"}"),
        enrichment_item("{\"echo\":\"b\"}"),
    ];
    enforce_token_budget(&mut items, 0);
    assert_eq!(item_text(&items[0]), "");
    assert_eq!(item_text(&items[1]), "");
}

#[test]
fn budget_is_consumed_in_item_order() {
    let first = "{\"a\":1}";
    let long = "y".repeat(8_000);
    let mut items = vec![enrichment_item(first), enrichment_item(&long)];
    enforce_token_budget(&mut items, 4);
    // First item fits and stays intact.
    assert_eq!(item_text(&items[0]), first);
    // Second item only gets the leftover budget (or none).
    assert!(item_text(&items[1]).len() <= long.len());
}
