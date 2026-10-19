use crate::{context::Context, dom::document, reference::ActiveReference};
use leptos::prelude::*;
use review_core::*;

#[component]
pub(crate) fn FeedbackPanel(cx: Context) -> impl IntoView {
    let scroll = move |_| {
        if let Some(element) = document().get_element_by_id("feedback-panel") {
            cx.change(|ui| ui.feedback_scroll = element.scroll_top() as f64);
        }
    };
    let draft = move |event| cx.change(|ui| ui.feedback_draft = event_target_value(&event));
    let key = move |event| cx.composer_key(event, true);
    let record = move |_| cx.record_feedback();
    view! {
        <div class="feedback-panel" id="feedback-panel" on:scroll=scroll>
            <h2>"Recorded feedback"</h2><p class="muted">"Your wording, collected as Markdown when you submit."</p>
            <For each=move || cx.review.get().map(|r| r.feedback).unwrap_or_default() key=|feedback| format!("{}:{}", feedback.id, feedback.text)
                children=move |feedback| view! { <FeedbackCard cx=cx feedback=feedback/> }/>
        </div>
        <div class="composer feedback-composer">
            <ActiveReference cx=cx/>
            <textarea aria-label="Feedback comment" placeholder="Write feedback for the handover…" prop:value=move || cx.ui.get().feedback_draft
                on:input=draft on:keydown=key disabled=move || cx.disabled()></textarea>
            <div class="composer-actions"><span class="muted">"Enter to record · Shift+Enter for a new line"</span>
                <button class="primary" disabled=move || cx.disabled() || cx.ui.get().feedback_draft.trim().is_empty() on:click=record>"Record feedback"</button>
            </div>
        </div>
    }
}

#[component]
fn FeedbackCard(cx: Context, feedback: Feedback) -> impl IntoView {
    let original = feedback.clone();
    let id = feedback.id.clone();
    let edit = move |_| cx.edit_feedback(original.clone());
    let delete = move |_| cx.send(Command::DeleteFeedback { id: id.clone() });
    view! {
        <article class="feedback-card">
            <small>{feedback.reference.as_ref().map(|r| r.label()).unwrap_or_else(|| "Overall feedback".into())}</small>
            <p>{feedback.text}</p>
            <button disabled=move || cx.disabled() on:click=edit>"Edit"</button>
            <button disabled=move || cx.disabled() on:click=delete>"Delete"</button>
        </article>
    }
}
