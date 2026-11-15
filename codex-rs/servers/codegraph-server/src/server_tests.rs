use super::*;
use pretty_assertions::assert_eq;

/// Every advertised tool must have a rich description from the table, and the
/// core navigation/discovery surface must be present.
#[test]
fn codegraph_tools_all_have_rich_descriptions() {
    let tools = codegraph_tools();
    assert!(!tools.is_empty());
    for tool in &tools {
        assert!(
            tool.description
                .as_ref()
                .is_some_and(|d| d.trim().len() > 40),
            "tool `{}` has no rich description",
            tool.name
        );
    }
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "search_symbol",
        "callers",
        "callees",
        "context",
        "impact",
        "mermaid",
        "begin",
        "status",
        "doc_search",
        "doc_hydrate",
        "doc_struct",
        "doc_mine_patterns",
        "doc_list_patterns",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    // The verb-style entry point and aliases are gone — flat tools replace them.
    for removed in ["call", "code_search", "code_stats"] {
        assert!(
            !names.contains(&removed),
            "tool {removed} should be removed"
        );
    }
}

#[test]
fn descriptions_are_unique_per_tool() {
    let tools = codegraph_tools();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(
        names.len(),
        count,
        "duplicate tool names in codegraph_tools()"
    );
}
