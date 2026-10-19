use crate::{Reference, Review};

pub fn export_markdown(review: &Review) -> String {
    let mut out = format!("# Review feedback\n\nReview: {}\n\n", review.id);
    for feedback in &review.feedback {
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
            None => out.push_str("## Overall feedback\n\n"),
        }
        out.push_str(&feedback.text);
        out.push_str("\n\n");
    }
    out
}
