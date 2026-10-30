//! Theme-derived styling for the configurable footer statusline.

use ratatui::prelude::Stylize;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::status_line_setup::StatusLineItem;

const STATUS_LINE_SEPARATOR: &str = " · ";

pub(crate) fn status_line_from_segments<I>(
    segments: I,
    use_theme_colors: bool,
) -> Option<Line<'static>>
where
    I: IntoIterator<Item = (StatusLineItem, String)>,
{
    let mut spans = Vec::new();
    for (item, text) in segments {
        if !spans.is_empty() {
            spans.push(STATUS_LINE_SEPARATOR.dim());
        }
        let _ = use_theme_colors;
        let style = Style::default().dim();
        let style = if item == StatusLineItem::PullRequestNumber {
            style.underlined()
        } else {
            style
        };
        spans.push(Span::styled(text, style));
    }

    (!spans.is_empty()).then(|| Line::from(spans))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::style::Modifier;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn status_line_segments_preserve_order_and_plain_text() {
        let line = status_line_from_segments(
            [
                (StatusLineItem::ModelName, "gpt-5".to_string()),
                (StatusLineItem::CurrentDir, "/repo".to_string()),
                (StatusLineItem::GitBranch, "main".to_string()),
            ],
            /*use_theme_colors*/ true,
        )
        .expect("status line");

        assert_eq!(line_text(&line), "gpt-5 · /repo · main");
        assert_eq!(line.spans[0].style.fg, None);
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(line.spans[2].style.fg, None);
        assert!(line.spans[2].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(line.spans[4].style.fg, None);
        assert!(line.spans[4].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn status_line_segments_dim_separators_and_ignore_theme_colors() {
        let line = status_line_from_segments(
            [
                (StatusLineItem::ModelName, "gpt-5".to_string()),
                (StatusLineItem::ContextUsed, "Context 12% used".to_string()),
            ],
            /*use_theme_colors*/ true,
        )
        .expect("status line");

        assert_eq!(line.spans[0].style.fg, None);
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(line.spans[2].style.fg, None);
        assert!(line.spans[2].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn status_line_segments_can_disable_theme_colors() {
        let line = status_line_from_segments(
            [
                (StatusLineItem::ModelName, "gpt-5".to_string()),
                (StatusLineItem::ContextUsed, "Context 12% used".to_string()),
            ],
            /*use_theme_colors*/ false,
        )
        .expect("status line");

        assert_eq!(line_text(&line), "gpt-5 · Context 12% used");
        assert_eq!(line.spans[0].style.fg, None);
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(line.spans[2].style.fg, None);
        assert!(line.spans[2].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn pull_request_number_uses_link_style() {
        let line = status_line_from_segments(
            [(StatusLineItem::PullRequestNumber, "PR #20252".to_string())],
            /*use_theme_colors*/ false,
        )
        .expect("status line");

        assert_eq!(line.spans[0].style.fg, None);
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(
            line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn status_line_segments_return_none_when_empty() {
        assert_eq!(
            status_line_from_segments(
                Vec::<(StatusLineItem, String)>::new(),
                /*use_theme_colors*/ true,
            ),
            None
        );
    }
}
