use crate::{Feedback, FeedbackCategory, Reference, Review};

pub fn export_markdown(review: &Review) -> String {
    export_feedback(&review.ui.feedback.overall, &review.feedback)
}

fn export_feedback(overall: &str, feedback: &[Feedback]) -> String {
    let mut out = String::new();
    if !overall.trim().is_empty() {
        out.push_str("# Overall feedback\n\n");
        out.push_str(overall);
        out.push_str("\n\n");
    }
    for (category, heading, intent) in [
        (
            FeedbackCategory::Change,
            "Changes",
            "Requested changes to implement.",
        ),
        (
            FeedbackCategory::Question,
            "Questions",
            "Questions to answer and discuss or plan; do not implement without a decision.",
        ),
    ] {
        let comments: Vec<_> = feedback.iter().filter(|f| f.category == category).collect();
        if comments.is_empty() {
            continue;
        }
        out.push_str(&format!("# {heading}\n\n{intent}\n\n"));
        for feedback in comments {
            match &feedback.reference {
                Some(reference @ Reference::Code { .. }) => {
                    out.push_str(&format!("## {}\n\n", reference.label()));
                }
                Some(Reference::Prose { quote }) => {
                    out.push_str("## Guide feedback\n\n");
                    for line in quote.lines() {
                        out.push_str(&format!("> {line}\n"));
                    }
                    out.push('\n');
                }
                None => {}
            }
            out.push_str(&feedback.text);
            out.push_str("\n\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiffSide;

    #[test]
    fn separates_intent_and_preserves_wording_references_and_order() {
        let code = Some(Reference::Code {
            file: "src/main.rs".into(),
            start: 2,
            end: 4,
            side: DiffSide::New,
        });
        let feedback = vec![
            Feedback {
                id: "q".into(),
                category: FeedbackCategory::Question,
                prose_offset: Some(0),
                code_block: None,
                reference: Some(Reference::Prose {
                    quote: "Line one\nLine two".into(),
                }),
                text: "Why this approach?".into(),
            },
            Feedback {
                id: "c".into(),
                category: FeedbackCategory::Change,
                prose_offset: None,
                code_block: None,
                reference: code.clone(),
                text: "Keep **my words**\nexactly.".into(),
            },
            Feedback {
                id: "c2".into(),
                category: FeedbackCategory::Change,
                prose_offset: None,
                code_block: None,
                reference: code,
                text: "Second change".into(),
            },
        ];
        assert_eq!(
            export_feedback("Overall assessment", &feedback),
            "# Overall feedback\n\nOverall assessment\n\n# Changes\n\nRequested changes to implement.\n\n## src/main.rs:2-4 (new)\n\nKeep **my words**\nexactly.\n\n## src/main.rs:2-4 (new)\n\nSecond change\n\n# Questions\n\nQuestions to answer and discuss or plan; do not implement without a decision.\n\n## Guide feedback\n\n> Line one\n> Line two\n\nWhy this approach?\n\n"
        );
    }

    #[test]
    fn omits_empty_sections() {
        assert_eq!(
            export_feedback("Looks good", &[]),
            "# Overall feedback\n\nLooks good\n\n"
        );
        assert_eq!(export_feedback("  ", &[]), "");
    }
}
