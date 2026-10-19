use crate::{Feedback, Reference, Review};

pub fn export_markdown(review: &Review) -> String {
    export_feedback(&review.feedback)
}

fn export_feedback(feedback: &[Feedback]) -> String {
    let mut out = String::new();
    let overall: Vec<_> = feedback.iter().filter(|f| f.reference.is_none()).collect();
    if !overall.is_empty() {
        out.push_str("# Overall feedback\n\n");
        for feedback in overall {
            out.push_str(&feedback.text);
            out.push_str("\n\n");
        }
    }
    let comments: Vec<_> = feedback.iter().filter(|f| f.reference.is_some()).collect();
    if !comments.is_empty() {
        out.push_str("# Detailed feedback\n\n");
    }
    for feedback in comments {
        match &feedback.reference {
            Some(Reference::Code { .. }) => out.push_str(&format!(
                "## {}\n\n",
                feedback.reference.as_ref().unwrap().label()
            )),
            Some(Reference::Prose { quote }) => {
                out.push_str("## Guide feedback\n\n");
                for line in quote.lines() {
                    out.push_str(&format!("> {line}\n"));
                }
                out.push('\n');
            }
            None => unreachable!("Only referenced comments are included here"),
        }
        out.push_str(&feedback.text);
        out.push_str("\n\n");
    }
    if out.is_empty() {
        out.push_str("# Review feedback\n\nNo feedback recorded.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiffSide;

    fn comment(text: &str, reference: Option<Reference>) -> Feedback {
        Feedback {
            id: text.into(),
            text: text.into(),
            reference,
        }
    }

    #[test]
    fn groups_overall_comments_before_references_without_rewriting_them() {
        let feedback = vec![
            comment(
                "Code comment",
                Some(Reference::Code {
                    file: "src/main.rs".into(),
                    start: 2,
                    end: 4,
                    side: DiffSide::New,
                }),
            ),
            comment("First overall\n**Keep my formatting**", None),
            comment(
                "Prose comment",
                Some(Reference::Prose {
                    quote: "Line one\nLine two".into(),
                }),
            ),
            comment("Second overall", None),
        ];
        assert_eq!(
            export_feedback(&feedback),
            "# Overall feedback\n\nFirst overall\n**Keep my formatting**\n\nSecond overall\n\n# Detailed feedback\n\n## src/main.rs:2-4 (new)\n\nCode comment\n\n## Guide feedback\n\n> Line one\n> Line two\n\nProse comment\n\n"
        );
    }

    #[test]
    fn omits_empty_sections() {
        assert_eq!(
            export_feedback(&[comment("Looks good", None)]),
            "# Overall feedback\n\nLooks good\n\n"
        );
        assert!(
            !export_feedback(&[comment(
                "Explain",
                Some(Reference::Prose {
                    quote: "Why".into()
                })
            )])
            .contains("Overall feedback")
        );
        assert_eq!(
            export_feedback(&[]),
            "# Review feedback\n\nNo feedback recorded.\n"
        );
    }
}
