use pulldown_cmark::{Event, Options, Parser, html};

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn markdown(s: &str) -> String {
    // No agent-provided HTML, images, or active links. Source references have their own controls.
    let events = Parser::new_ext(
        s,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_DEFINITION_LIST,
    )
    .filter_map(|e| match e {
        Event::Html(_) | Event::InlineHtml(_) => None,
        Event::Start(pulldown_cmark::Tag::Link { .. } | pulldown_cmark::Tag::Image { .. })
        | Event::End(pulldown_cmark::TagEnd::Link | pulldown_cmark::TagEnd::Image) => None,
        _ => Some(e),
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}
pub fn plain_text(s: &str) -> String {
    Parser::new_ext(s, Options::ENABLE_DEFINITION_LIST)
        .filter_map(|e| match e {
            Event::Text(t) | Event::Code(t) => Some(t.to_string()),
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(
                pulldown_cmark::TagEnd::Paragraph
                | pulldown_cmark::TagEnd::Heading(_)
                | pulldown_cmark::TagEnd::CodeBlock
                | pulldown_cmark::TagEnd::Item
                | pulldown_cmark::TagEnd::DefinitionListTitle
                | pulldown_cmark::TagEnd::DefinitionListDefinition,
            ) => Some("\n".into()),
            _ => None,
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn definition_lists_render_and_remain_selectable() {
        let guide = "Snapshot\n: Captured source, fixed during the review.\n";
        let html = markdown(guide);
        assert!(html.contains("<dt>Snapshot</dt>"));
        assert!(html.contains("<dd>"));
        assert!(
            crate::validate_reference(
                &crate::Reference::Prose {
                    quote: "Captured source, fixed during the review.".into()
                },
                &crate::Workspace {
                    fingerprint: String::new(),
                    diff: String::new(),
                    vcs: "git".into(),
                    files: vec![]
                },
                guide,
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_active_content() {
        let h = markdown(
            "<script>alert(1)</script>\n\n[click](javascript:alert(1))\n\n![x](https://tracker)",
        );
        assert!(!h.contains("<script"));
        assert!(!h.contains("href="));
        assert!(!h.contains("<img"));
    }
}
