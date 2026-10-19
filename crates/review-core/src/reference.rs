use crate::{DiffSide, Workspace, document};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reference {
    Code {
        file: String,
        start: u32,
        end: u32,
        side: DiffSide,
    },
    Prose {
        quote: String,
    },
}
impl Reference {
    pub fn label(&self) -> String {
        match self {
            Self::Code {
                file,
                start,
                end,
                side,
            } => format!("{file}:{start}-{end} ({side})"),
            Self::Prose { quote } => format!("Guide: {quote}"),
        }
    }
}
pub fn validate_reference(
    reference: &Reference,
    workspace: &Workspace,
    guide: &str,
) -> Result<(), String> {
    match reference {
        Reference::Prose { quote } => {
            if quote.trim().is_empty() {
                return Err("Select some guide text first".into());
            }
            // Browser text differs from Markdown markup; compare rendered text as well.
            let plain = document::plain_text(guide);
            let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            if !normalize(&plain).contains(&normalize(quote)) {
                return Err("Selected text is not in this guide".into());
            }
        }
        Reference::Code {
            file,
            start,
            end,
            side,
        } => {
            let f = workspace
                .files
                .iter()
                .find(|f| &f.path == file)
                .ok_or("File is not in the reviewed changes")?;
            let source = match side {
                DiffSide::Old => &f.old,
                DiffSide::New => &f.new,
            };
            let count = source.as_ref().map(|s| s.lines().count()).unwrap_or(0);
            if *start == 0 || end < start || *end as usize > count {
                return Err("Line range is outside the reviewed file".into());
            }
        }
    }
    Ok(())
}

impl Reference {
    /// The immutable content behind the reference, for both chat display and the harness.
    pub fn quote(&self, workspace: &Workspace) -> String {
        match self {
            Self::Prose { quote } => quote.clone(),
            Self::Code {
                file,
                start,
                end,
                side,
            } => workspace
                .files
                .iter()
                .find(|source| &source.path == file)
                .and_then(|source| match side {
                    DiffSide::Old => source.old.as_ref(),
                    DiffSide::New => source.new.as_ref(),
                })
                .map(|source| {
                    source
                        .lines()
                        .skip(start.saturating_sub(1) as usize)
                        .take(end.saturating_sub(*start) as usize + 1)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
        }
    }

    pub fn context(&self, workspace: &Workspace) -> String {
        match self {
            Self::Prose { .. } => self.label(),
            Self::Code { .. } => format!("{}\n\n{}", self.label(), self.quote(workspace)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_quotes_the_captured_side_and_selected_lines() {
        let workspace = Workspace {
            fingerprint: String::new(),
            vcs: "git".into(),
            diff: String::new(),
            files: vec![crate::SourceFile {
                path: "a.rs".into(),
                old: Some("before\nold line\nend\n".into()),
                new: Some("after\nnew line\nend\n".into()),
            }],
        };
        let reference = Reference::Code {
            file: "a.rs".into(),
            start: 2,
            end: 3,
            side: DiffSide::Old,
        };
        assert_eq!(reference.quote(&workspace), "old line\nend");
        assert_eq!(
            reference.context(&workspace),
            "a.rs:2-3 (old)\n\nold line\nend"
        );
    }
}
