use arborium::{
    GrammarStore, Highlighter, HtmlFormat,
    advanced::{CompiledGrammar, GrammarConfig, spans_to_html},
};
use review_core::{DiffSide, SourceFile, document::escape};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

// Keep expensive query compilation across guide generation and review restoration.
static GRAMMARS: LazyLock<Arc<GrammarStore>> = LazyLock::new(|| {
    let store = GrammarStore::new();
    // Arborium's bundled Markdown omits inline parsing. Register the upstream paired
    // grammars so emphasis, links, inline code, and fenced-language injections work.
    for (name, language, highlights, injections) in [
        (
            "markdown",
            tree_sitter_md::LANGUAGE,
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            tree_sitter_md::INJECTION_QUERY_BLOCK,
        ),
        (
            "markdown_inline",
            tree_sitter_md::INLINE_LANGUAGE,
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
            tree_sitter_md::INJECTION_QUERY_INLINE,
        ),
    ] {
        if let Ok(grammar) = CompiledGrammar::new(GrammarConfig {
            language: language.into(),
            highlights_query: highlights,
            injections_query: injections,
            locals_query: "",
        }) {
            store.insert(name, Arc::new(grammar));
        }
    }
    Arc::new(store)
});
const MAX_SOURCE_BYTES: usize = 512 * 1024;
const MAX_REVIEW_BYTES: usize = 4 * 1024 * 1024;

/// One render's cache. The snapshots are immutable; repeated excerpts reuse their lines.
pub(crate) struct Syntax<'a> {
    engine: Highlighter,
    sources: HashMap<(String, bool, String), Source<'a>>,
    remaining_bytes: usize,
}

pub(crate) struct Source<'a> {
    lines: Vec<&'a str>,
    highlighted: Option<Vec<String>>,
}

impl Source<'_> {
    pub fn text(&self, number: u32) -> Option<&str> {
        self.lines.get(number.checked_sub(1)? as usize).copied()
    }

    pub fn html(&self, number: u32) -> Option<String> {
        let text = self.text(number)?;
        Some(
            self.highlighted
                .as_ref()
                .map_or_else(|| escape(text), |lines| lines[number as usize - 1].clone()),
        )
    }
}

impl<'a> Syntax<'a> {
    pub fn new() -> Self {
        Self {
            engine: Highlighter::with_store(GRAMMARS.clone()),
            sources: HashMap::new(),
            remaining_bytes: MAX_REVIEW_BYTES,
        }
    }

    pub fn language(path: &str, fence: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        let detected = if name.ends_with(".tf.json") || name.ends_with(".tfvars.json") {
            Some("json")
        } else if name.ends_with(".tfvars") {
            Some("hcl")
        } else if name == "dockerfile" || name.starts_with("dockerfile.") {
            Some("dockerfile")
        } else if matches!(name.as_str(), "makefile" | "gnumakefile") {
            Some("make")
        } else {
            arborium::detect_language(&name)
        };
        detected.unwrap_or(fence).to_ascii_lowercase()
    }

    pub fn source(
        &mut self,
        file: &'a SourceFile,
        side: DiffSide,
        language: &str,
    ) -> Option<&Source<'a>> {
        let old = side == DiffSide::Old;
        let key = (file.path.clone(), old, language.to_string());
        if !self.sources.contains_key(&key) {
            let text = if old {
                file.old.as_deref()?
            } else {
                file.new.as_deref()?
            };
            let highlighted = self.highlight_lines(language, text);
            self.sources.insert(
                key.clone(),
                Source {
                    lines: text.lines().collect(),
                    highlighted,
                },
            );
        }
        self.sources.get(&key)
    }

    /// Unmapped fences can be coloured but never gain a verified source reference.
    pub fn snippet(&mut self, language: &str, text: &str) -> String {
        match self.highlight_lines(language, text) {
            Some(lines) => {
                let mut html = lines.join("\n");
                if text.ends_with('\n') {
                    html.push('\n');
                }
                html
            }
            None => escape(text),
        }
    }

    fn highlight_lines(&mut self, language: &str, text: &str) -> Option<Vec<String>> {
        if text.len() > MAX_SOURCE_BYTES || text.len() > self.remaining_bytes {
            return None;
        }
        self.engine.store().get(language)?;
        self.remaining_bytes -= text.len();
        let spans = self.engine.highlight_spans(language, text).ok()?;
        let lines: Vec<_> = text.lines().collect();
        let offsets: Vec<_> = lines
            .iter()
            .map(|line| line.as_ptr() as usize - text.as_ptr() as usize)
            .collect();
        let mut line_spans = vec![Vec::new(); lines.len()];

        // Clip multiline tokens to each row, preserving complete, independently valid HTML.
        // Locate the first row by binary search instead of scanning every span for every line.
        for span in spans {
            let first = offsets
                .partition_point(|offset| *offset <= span.start as usize)
                .saturating_sub(1);
            for index in first..lines.len() {
                let offset = offsets[index];
                if offset >= span.end as usize {
                    break;
                }
                let start = (span.start as usize).max(offset);
                let end = (span.end as usize).min(offset + lines[index].len());
                if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                    let mut clipped = span.clone();
                    clipped.start = (start - offset) as u32;
                    clipped.end = (end - offset) as u32;
                    line_spans[index].push(clipped);
                }
            }
        }

        let format = HtmlFormat::ClassNamesWithPrefix("syntax".into());
        Some(
            lines
                .into_iter()
                .zip(line_spans)
                .map(|(line, spans)| spans_to_html(line, spans, &format))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Sample {
        path: String,
        language: String,
        source: String,
        category: String,
        token: String,
    }

    #[test]
    fn requested_languages_have_meaningful_highlights() {
        let samples: Vec<Sample> =
            serde_json::from_str(include_str!("../../../tests/fixtures/syntax.json")).unwrap();
        let mut syntax = Syntax::new();
        for sample in samples {
            assert_eq!(Syntax::language(&sample.path, "text"), sample.language);
            let html = syntax.snippet(&sample.language, &sample.source);
            let expected = format!("class=\"syntax-{}\">", sample.category);
            assert!(
                html.split(&expected).skip(1).any(|part| part
                    .split("</span>")
                    .next()
                    .unwrap()
                    .contains(&sample.token)),
                "{}: {html}",
                sample.path
            );
        }
    }

    #[test]
    fn multiline_tokens_keep_context_and_safe_unicode_text() {
        let file = SourceFile {
            path: "a.rs".into(),
            old: None,
            new: Some("/* first\nsecond café 🦀 <script> &\nlast */\nfn main() {}\n".into()),
        };
        let mut syntax = Syntax::new();
        let source = syntax.source(&file, DiffSide::New, "rust").unwrap();
        assert_eq!(source.text(2), Some("second café 🦀 <script> &"));
        assert_eq!(
            source.html(2).unwrap(),
            "<span class=\"syntax-comment\">second café 🦀 &lt;script&gt; &amp;</span>"
        );
        assert!(source.html(4).unwrap().contains("syntax-keyword"));
        assert!(source.html(0).is_none());
    }

    #[test]
    fn aliases_unknown_languages_limits_and_cache() {
        assert_eq!(Syntax::language("infra/vars.tfvars", "text"), "hcl");
        assert_eq!(Syntax::language("infra/main.tf.json", "terraform"), "json");
        assert_eq!(
            Syntax::language("build/Dockerfile.dev", "text"),
            "dockerfile"
        );
        assert_eq!(Syntax::language("build/Makefile", "text"), "make");
        assert_eq!(Syntax::language("unknown", "TS"), "ts");
        let mut syntax = Syntax::new();
        assert_eq!(
            syntax.snippet("unknown", "<script>&"),
            "&lt;script&gt;&amp;"
        );
        assert!(
            syntax
                .snippet("ts", "const x = 1;")
                .contains("syntax-keyword")
        );
        assert!(
            !syntax
                .snippet("rust", &"x".repeat(MAX_SOURCE_BYTES + 1))
                .contains("<span")
        );

        let file = SourceFile {
            path: "a.rs".into(),
            old: None,
            new: Some("fn main() {}\n".into()),
        };
        syntax.source(&file, DiffSide::New, "rust").unwrap();
        let remaining = syntax.remaining_bytes;
        syntax.source(&file, DiffSide::New, "rust").unwrap();
        assert_eq!(syntax.remaining_bytes, remaining);
        syntax.remaining_bytes = 0;
        assert_eq!(syntax.snippet("rust", "fn main() {}"), "fn main() {}");
    }

    #[test]
    fn markdown_and_html_embedded_languages_are_highlighted() {
        let mut syntax = Syntax::new();
        for (language, source) in [
            ("markdown", "# Example\n\n```rust\nfn main() {}\n```\n"),
            ("html", "<script>const value = 42;</script>"),
        ] {
            let html = syntax.snippet(language, source);
            assert!(html.contains("syntax-keyword"), "{language}: {html}");
        }
        let html = syntax.snippet(
            "markdown",
            "A **bold** change with `inline code` and [link](https://example.com).\n",
        );
        assert!(html.contains("syntax-strong"), "{html}");
        assert!(html.contains("syntax-literal"), "{html}");
        assert!(html.contains("syntax-link"), "{html}");
    }

    #[test]
    fn crlf_and_empty_lines_keep_source_line_numbers() {
        let file = SourceFile {
            path: "a.rs".into(),
            old: None,
            new: Some("/*\r\n\r\nhello\r\n*/\r\n".into()),
        };
        let mut syntax = Syntax::new();
        let source = syntax.source(&file, DiffSide::New, "rust").unwrap();
        assert_eq!(source.html(2).unwrap(), "");
        assert_eq!(
            source.html(3).unwrap(),
            "<span class=\"syntax-comment\">hello</span>"
        );
        assert_eq!(source.lines.len(), 4);
    }
}
