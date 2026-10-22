use crate::{
    app::{Shared, publish},
    session::spawn_turn,
};
use anyhow::{Result, bail};
use review_core::*;
use uuid::Uuid;

pub(crate) async fn handle(app: &Shared, c: Envelope) -> Result<()> {
    let mut s = app.store.lock().await;
    if c.review_id != s.review.id {
        bail!(
            "The workspace changed. This action belongs to the archived review; review the fresh snapshot before retrying."
        );
    }
    // Check ownership before deduplication: an old page must not act on a new review.
    if s.seen(&c.id)? {
        return Ok(());
    }
    let before = s.review.clone();

    if s.review.status == ReviewStatus::Completed
        && !matches!(c.command, Command::SaveUi { .. } | Command::Conclude)
    {
        bail!("This review has been submitted.");
    }
    let mut launch = None;
    let mut response = None;
    let mut interrupt = false;
    match &c.command {
        Command::SaveUi { ui } => s.review.ui = *ui.clone(),
        Command::Feedback {
            id,
            category,
            prose_offset,
            code_block,
            text,
            reference,
        } => {
            if text.trim().is_empty() {
                bail!("Write feedback first");
            }
            if let Some(r) = reference {
                validate_reference(r, &s.review.workspace, &s.review.guide)
                    .map_err(anyhow::Error::msg)?;
            }
            let f = Feedback {
                id: id.clone(),
                category: *category,
                prose_offset: *prose_offset,
                code_block: *code_block,
                text: text.clone(),
                reference: reference.clone(),
            };
            if let Some(existing) = s.review.feedback.iter_mut().find(|f| &f.id == id) {
                *existing = f;
            } else {
                s.review.feedback.push(f);
            }
            s.review.ui.feedback.drafts.remove(id);
        }
        Command::DeleteFeedback { id } => {
            s.review.feedback.retain(|f| &f.id != id);
            s.review.ui.feedback.drafts.remove(id);
        }
        Command::Chat { text, reference } => {
            if s.review.busy {
                bail!("Codex is still working; stop it or wait before sending");
            }
            if text.trim().is_empty() {
                bail!("Write a question first");
            }
            if let Some(r) = reference {
                validate_reference(r, &s.review.workspace, &s.review.guide)
                    .map_err(anyhow::Error::msg)?;
            }
            s.review.messages.push(review_core::Message {
                id: c.id.clone(),
                role: MessageRole::User,
                text: text.clone(),
                reference: reference.clone(),
                status: MessageStatus::Complete,
            });
            s.review.ui.chat.draft.clear();
            launch = Some(Some(format!(
                "{}{}",
                text,
                reference
                    .as_ref()
                    .map(|r| format!(
                        "\n\nSelected review reference:\n{}",
                        r.context(&s.review.workspace)
                    ))
                    .unwrap_or_default()
            )));
        }
        Command::Generate => {
            if !s.review.guide.is_empty() {
                bail!("The walkthrough is already prepared for this review");
            }
            if s.review.busy {
                bail!("Codex is already working");
            }
            launch = Some(None);
        }
        Command::Interrupt => interrupt = true,
        Command::Respond {
            request_id,
            response: r,
        } => {
            if !s.review.requests.iter().any(|x| x.id == *request_id) {
                bail!("This request is no longer pending");
            }
            response = Some((request_id.clone(), r.clone()));
        }
        Command::Conclude => s.conclude()?,
    }
    if let Some(ref prompt) = launch {
        s.review.busy = true;
        s.review.generating = prompt.is_none();
        if s.review.generating {
            s.review.preparation_started_at_ms = Some(crate::progress::now_ms());
        }
        s.review.error = None;
    }
    // Approval replies must reach the live harness before acknowledging the action.
    // Failed replies remain pending and can be retried.
    if response.is_some() || interrupt {
        s.save("command_pending", None)?;
    } else if let Err(error) = s.save("command", Some(&c)) {
        s.review = before;
        return Err(error);
    }
    publish(app, &s);
    drop(s);
    if let Some((id, r)) = response {
        if let Some(a) = app.agent.lock().await.clone() {
            a.respond(id.clone(), r).await?;
        } else if !app.fixture {
            bail!("Codex disconnected. Interrupt and retry the turn.");
        }
        let mut s = app.store.lock().await;
        s.review.requests.retain(|x| x.id != id);
        if app.fixture {
            s.review.busy = false;
            s.review.messages.push(review_core::Message {
                id: Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                text: "Fixture request answered.".into(),
                reference: None,
                status: MessageStatus::Complete,
            });
        }
        s.save("response", Some(&c))?;
        publish(app, &s);
    }
    if interrupt {
        let state = app.store.lock().await.review.clone();
        if let Some(a) = app.agent.lock().await.clone() {
            if let (Some(thread), Some(turn)) = (state.thread_id, state.turn_id) {
                a.interrupt(&thread, &turn).await?;
            } else {
                a.stop().await;
            }
        }
        let mut s = app.store.lock().await;
        if app.fixture || s.review.turn_id.is_none() {
            s.review.busy = false;
            s.review.generating = false;
            s.review.requests.clear();
            for m in &mut s.review.messages {
                if m.status == MessageStatus::Streaming {
                    m.status = MessageStatus::Interrupted;
                }
            }
        }
        s.save("interrupt", Some(&c))?;
        publish(app, &s);
    }
    if let Some(prompt) = launch {
        spawn_turn(app, prompt);
    }
    Ok(())
}
