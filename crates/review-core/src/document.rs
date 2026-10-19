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
        // An agent may quote current source instead of a diff. For modified files,
        // prefer the actual captured hunks touching that excerpt, including deletions.
        if source.old.is_some() {
            let end = m
                .start
                .checked_add(content.lines().count() as u32)?
                .checked_sub(1)?;
            if let Some(patch) = overlapping_hunks(&workspace.diff, &m.file, m.start, end) {
                return render_code(
                    &Meta {
                        file: m.file.clone(),
                        start: 0,
                        diff: true,
                    },
                    &patch,
                    workspace,
                    block,
                );
            }
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
            (
                None,
                Some(n),
                line,
                if source.old.is_none() {
                    "added"
                } else {
                    "context"
                },
            )
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
    let ranges = [
        ("old", old_range),
        (if m.diff { "new" } else { "lines" }, new_range),
    ]
    .into_iter()
    .filter_map(|(side, range)| range.map(|(start, end)| format!("{side} {start}–{end}")))
    .collect::<Vec<_>>()
    .join(" · ");
    let kind = if m.diff {
        "diff"
    } else if source.old.is_none() {
        "new file"
    } else {
        "current code"
    };
    let (side, (start, end)) = new_range
        .map(|range| ("new", range))
        .or_else(|| old_range.map(|range| ("old", range)))?;
    Some(format!(
        "<section class=\"code-block\"><header><span class=\"code-reference\" data-file=\"{file}\" data-start=\"{start}\" data-end=\"{end}\" data-side=\"{side}\">{file} <small>{kind} · {ranges}</small></span><button data-copy=\"{file}:{first}\" title=\"Copy source reference\">Copy reference</button></header><div class=\"code-rows\">{rows}</div></section>",
        file = escape(&m.file),
    ))
}

/// Keep complete VCS hunks so replacements and deletions retain their true sides.
fn overlapping_hunks(diff: &str, file: &str, start: u32, end: u32) -> Option<String> {
    let header = format!("diff --git a/{file} b/{file}");
    let mut in_file = false;
    let mut include = false;
    let mut selected = String::new();
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            in_file = line == header;
            include = false;
        } else if in_file && line.starts_with("@@ ") {
            let range = line.split_whitespace().nth(2)?.strip_prefix('+')?;
            let (line_start, count) = range.split_once(',').unwrap_or((range, "1"));
            let line_start: u32 = line_start.parse().ok()?;
            let count: u32 = count.parse().ok()?;
            let line_end = line_start.saturating_add(count.saturating_sub(1));
            include = line_start <= end && line_end >= start;
            if include {
                selected.push_str(line);
                selected.push('\n');
            }
        } else if in_file
            && include
            && matches!(line.as_bytes().first(), Some(b'+' | b'-' | b' ' | b'\\'))
        {
            selected.push_str(line);
            selected.push('\n');
        }
    }
    (!selected.is_empty()).then_some(selected)
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
    fn current_excerpt_of_modified_file_uses_the_captured_diff() {
        let mut w = workspace();
        w.diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n same\n".into();
        let guide = "```rust {\"file\":\"a.rs\",\"start\":1}\nnew\nsame\n```";
        let (html, warnings) = render(guide, &w);
        assert!(warnings.is_empty());
        assert!(html.contains("code-line removed"));
        assert!(html.contains("code-line added"));
        assert!(html.contains("code-line context"));
        assert!(html.contains("diff · old 1–2 · new 1–2"));
        assert!(!html.contains("new file"));
        assert!(html.contains("data-start=\"1\" data-end=\"2\" data-side=\"new\""));
        assert!(
            !render(&guide.replace("new\nsame", "fake\nsame"), &w)
                .1
                .is_empty()
        );
    }

    #[test]
    fn excerpt_hunks_are_scoped_to_file_and_range_including_deletions() {
        let diff = "diff --git a/other.rs b/other.rs\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/a.rs b/a.rs\n@@ -1,2 +1,1 @@\n-remove\n keep\n@@ -20 +19 @@\n-before\n+after\n";
        let selected = overlapping_hunks(diff, "a.rs", 1, 1).unwrap();
        assert!(selected.contains("-remove"));
        assert!(!selected.contains("-x"));
        assert!(!selected.contains("-before"));
        assert!(overlapping_hunks(diff, "a.rs", 8, 10).is_none());
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
