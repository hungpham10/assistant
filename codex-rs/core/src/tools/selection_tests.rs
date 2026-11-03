use super::*;
use pretty_assertions::assert_eq;

#[test]
fn set_enabled_moves_between_deny_and_enable() {
    let mut selection = SessionToolSelection::default();
    selection.set_enabled("apply_patch", false);
    assert!(selection.tool_is_hidden(&ToolName::plain("apply_patch")));

    selection.set_enabled("apply_patch", true);
    assert!(selection.enable.contains("apply_patch"));
    assert!(!selection.tool_is_hidden(&ToolName::plain("apply_patch")));
}

#[test]
fn namespace_wildcard_denies_whole_server() {
    let mut selection = SessionToolSelection::default();
    selection.set_enabled("chrome-devtools:*", false);

    assert!(selection.tool_is_hidden(
        &ToolName::namespaced("chrome-devtools", "click"),
    ));
    assert!(!selection.tool_is_hidden(&ToolName::plain("shell")));

    // Re-enabling one tool inside a denied namespace wins.
    selection.set_enabled("chrome-devtools:screenshot", true);
    assert!(!selection.tool_is_hidden(
        &ToolName::namespaced("chrome-devtools", "screenshot"),
    ));
    assert!(selection.tool_is_hidden(
        &ToolName::namespaced("chrome-devtools", "click"),
    ));
}

#[test]
fn plan_mode_demotes_write_builtin_tools_only() {
    let selection = SessionToolSelection::default();
    for name in [
        "shell",
        "unified_exec",
        "apply_patch",
        "Write",
        "Edit",
        "Bash",
    ] {
        assert!(
            selection.tool_is_demoted(&ToolName::plain(name), /*plan_mode*/ true),
            "{name} should be demoted in plan mode"
        );
    }
    assert!(!selection.tool_is_demoted(&ToolName::plain("web_search"), /*plan_mode*/ true));
    assert!(!selection.tool_is_demoted(&ToolName::plain("update_plan"), /*plan_mode*/ true));
    assert!(!selection.tool_is_demoted(&ToolName::plain("shell"), /*plan_mode*/ false));
}

#[test]
fn protected_tools_are_never_demoted() {
    let mut selection = SessionToolSelection::default();
    selection.set_enabled("update_plan", false);
    selection.set_enabled("request_user_input", false);
    assert!(!selection.tool_is_hidden(&ToolName::plain("update_plan")));
    assert!(!selection.tool_is_hidden(&ToolName::plain("request_user_input")));
    assert!(!selection.tool_is_demoted(&ToolName::plain("update_plan"), /*plan_mode*/ true));
    assert!(!selection.tool_is_demoted(
        &ToolName::plain("request_user_input"),
        /*plan_mode*/ false,
    ));
}

#[test]
fn display_key_matches_display_form() {
    assert_eq!(
        display_key(&ToolName::namespaced("server", "tool")),
        "server:tool"
    );
    assert_eq!(display_key(&ToolName::plain("shell")), "shell");
}
