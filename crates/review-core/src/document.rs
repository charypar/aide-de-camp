use crate::Workspace;
use pulldown_cmark::{Event, Options, Parser, html};
use serde::Deserialize;

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
#[derive(Deserialize)]
struct Meta {
    file: String,
    #[serde(default)]
    start: u32,
    #[serde(default)]
    diff: bool,
}

/// Fence: ```rust {"file":"src/main.rs","start":1} or diff:true with unified @@ hunks.
pub fn render(guide: &str, workspace: &Workspace) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut prose = String::new();
    let mut warnings = vec![];
    let mut lines = guide.lines().peekable();
    let mut block = 0;
    while let Some(line) = lines.next() {
        if let Some(info) = line.strip_prefix("```") {
            out.push_str(&format!(
                "<section class=\"prose\">{}</section>",
                markdown(&prose)
            ));
            prose.clear();
            let mut content = String::new();
            for l in lines.by_ref() {
                if l.starts_with("```") {
                    break;
                }
                content.push_str(l);
                content.push('\n');
            }
            let meta = info
                .find('{')
                .and_then(|i| serde_json::from_str::<Meta>(&info[i..]).ok());
            let rendered = meta
                .as_ref()
                .and_then(|m| render_code(m, &content, workspace, block));
            if let Some(code) = rendered {
                out.push_str(&code);
            } else {
                warnings.push(format!("Snippet {} has no verified source mapping; selection is disabled for this block.",block+1));
                out.push_str(&format!(
                    "<pre class=\"unmapped\"><code>{}</code></pre>",
                    escape(&content)
                ));
            }
            block += 1;
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    out.push_str(&format!(
        "<section class=\"prose\">{}</section>",
        markdown(&prose)
    ));
    (out, warnings)
}
fn render_code(m: &Meta, content: &str, workspace: &Workspace, block: usize) -> Option<String> {
    let source = workspace.files.iter().find(|f| f.path == m.file)?;
    let mut rows = String::new();
    let mut old = 0;
    let mut new = m.start;
    let mut first = 0;
    let mut old_range: Option<(u32, u32)> = None;
    let mut new_range: Option<(u32, u32)> = None;
    if !m.diff {
        if new == 0 {
            return None;
        }
        let actual = source
            .new
            .as_ref()?
            .lines()
            .skip((new - 1) as usize)
            .take(content.lines().count())
            .collect::<Vec<_>>()
            .join("\n");
        if actual != content.trim_end_matches('\n') {
            return None;
        }
    }
    for (i, line) in content.lines().enumerate() {
        if m.diff && line.starts_with("@@ ") {
            let parts: Vec<_> = line.split_whitespace().collect();
            old = parts
                .get(1)?
                .trim_start_matches('-')
                .split(',')
                .next()?
                .parse()
                .ok()?;
            new = parts
                .get(2)?
                .trim_start_matches('+')
                .split(',')
                .next()?
                .parse()
                .ok()?;
            rows.push_str(&format!("<div class=\"hunk\">{}</div>", escape(line)));
            continue;
        }
        let (o, n, text, class) = if m.diff {
            match line.chars().next()? {
                '+' => {
                    let n = new;
                    new += 1;
                    (None, Some(n), &line[1..], "added")
                }
                '-' => {
                    let o = old;
                    old += 1;
                    (Some(o), None, &line[1..], "removed")
                }
                ' ' => {
                    let o = old;
                    let n = new;
                    old += 1;
                    new += 1;
                    (Some(o), Some(n), &line[1..], "context")
                }
                '\\' => continue,
                _ => return None,
            }
        } else {
            let n = new;
            new += 1;
            (None, Some(n), line, "context")
        };
        for (number, src) in [(o, &source.old), (n, &source.new)] {
            if let Some(number) = number
                && (number == 0 || src.as_ref()?.lines().nth((number - 1) as usize)? != text)
            {
                return None;
            }
        }
        for (number, range) in [(o, &mut old_range), (n, &mut new_range)] {
            if let Some(number) = number {
                *range = Some(match *range {
                    Some((start, end)) => (start.min(number), end.max(number)),
                    None => (number, number),
                });
            }
        }
        if first == 0 {
            first = n.or(o)?;
        }
        let num = n.or(o)?;
        let side = if n.is_some() { "new" } else { "old" };
        rows.push_str(&format!("<div class=\"code-line {class}\" data-file=\"{}\" data-line=\"{num}\" data-side=\"{side}\" data-old-line=\"{}\" data-new-line=\"{}\" id=\"code-{block}-{i}\"><span class=\"line-number\">{}</span><span class=\"line-number\">{}</span><code>{}</code></div>",escape(&m.file),o.map(|v|v.to_string()).unwrap_or_default(),n.map(|v|v.to_string()).unwrap_or_default(),o.map(|v|v.to_string()).unwrap_or_default(),n.map(|v|v.to_string()).unwrap_or_default(),escape(text)));
    }
    if first == 0 {
        return None;
    }
    let ranges = [("old", old_range), ("new", new_range)]
        .into_iter()
        .filter_map(|(side, range)| range.map(|(start, end)| format!("{side} {start}–{end}")))
        .collect::<Vec<_>>()
        .join(" · ");
    Some(format!(
        "<section class=\"code-block\"><header><span>{} <small>{ranges}</small></span><button data-copy=\"{}:{first}\" title=\"Copy source reference\">Copy reference</button></header><div class=\"code-rows\">{rows}</div></section>",
        escape(&m.file),
        escape(&m.file)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> Workspace {
        Workspace {
            fingerprint: "x".into(),
            diff: "".into(),
            vcs: "git".into(),
            files: vec![crate::SourceFile {
                path: "a.rs".into(),
                old: Some("old\nsame\n".into()),
                new: Some("new\nsame\n".into()),
            }],
        }
    }
    #[test]
    fn maps_both_sides_and_rejects_fabrication() {
        let w = workspace();
        let guide = "# Why\n\n```rust {\"file\":\"a.rs\",\"diff\":true}\n@@ -1,2 +1,2 @@\n-old\n+new\n same\n```";
        let (h, warn) = render(guide, &w);
        assert!(warn.is_empty());
        assert!(h.contains("data-side=\"old\""));
        assert!(h.contains("data-line=\"2\""));
        assert!(!render(&guide.replace("+new", "+fake"), &w).1.is_empty());
    }
    #[test]
    fn definition_lists_render_and_remain_selectable() {
        let guide = "Snapshot\n: Captured source, fixed during the review.\n";
        let (html, warnings) = render(guide, &workspace());
        assert!(warnings.is_empty());
        assert!(html.contains("<dt>Snapshot</dt>"));
        assert!(html.contains("<dd>"));
        assert!(
            crate::validate_reference(
                &crate::Reference::Prose {
                    quote: "Captured source, fixed during the review.".into()
                },
                &workspace(),
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
