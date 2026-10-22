use crate::{context::Context, dom::focus};
use leptos::prelude::*;
use review_core::*;

#[component]
pub(crate) fn SelectionActions(cx: Context) -> impl IntoView {
    let feedback = move |_| cx.add_feedback();
    let discuss = move |_| cx.discuss_selection();
    let keep_selection = move |event: web_sys::MouseEvent| event.prevent_default();
    view! {
        <Show when=move || cx.ui.get().document.selection.is_some() && cx.ui.get().document.selection_actions>
            <div id="selection-actions" class="selection-actions" role="toolbar" aria-label="Selection actions" data-reference=move || cx.ui.get().document.selection.map(|r| r.label()).unwrap_or_default() on:mousedown=keep_selection>
                <button aria-label="Add feedback" title="Add feedback" on:click=feedback disabled=move || cx.disabled()>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 11a8 8 0 0 1-8 8H5l-4 3V11a8 8 0 0 1 8-8h4a8 8 0 0 1 8 8Z"/><path d="M8 11h8M12 7v8"/></svg>
                </button>
                <button aria-label="Discuss with agent" title="Discuss with agent" on:click=discuss disabled=move || cx.disabled()>
                    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12 2 2.5 7.5L22 12l-7.5 2.5L12 22l-2.5-7.5L2 12l7.5-2.5Z"/></svg>
                </button>
            </div>
        </Show>
    }
}

#[component]
pub(crate) fn FeedbackMargin(cx: Context) -> impl IntoView {
    view! {
        <section class="feedback-margin" aria-label="Inline feedback">
            <For each=move || cx.comments() key=|f| f.id.clone() children=move |feedback| view! { <FeedbackCard cx=cx id=feedback.id/> }/>
        </section>
    }
}

#[component]
fn FeedbackCard(cx: Context, id: String) -> impl IntoView {
    let key = StoredValue::new(id.clone());
    let current = Memo::new(move |_| cx.comments().into_iter().find(|f| f.id == key.get_value()));
    let editing = move || cx.ui.get().feedback.drafts.contains_key(&key.get_value());
    let focused = move || cx.ui.get().feedback.focused.as_ref() == Some(&key.get_value());
    let edit = move |_| {
        if let Some(f) = current.get_untracked() {
            cx.edit_feedback(f);
        }
    };
    let delete = move |_| cx.delete_feedback(key.get_value());
    let cancel = move |_| cx.discard_draft(&key.get_value());
    let draft = move |event| {
        cx.change(|ui| {
            if let Some(f) = ui.feedback.drafts.get_mut(&key.get_value()) {
                f.text = event_target_value(&event);
            }
        })
    };
    let activate = move |_| cx.change(|ui| ui.feedback.focused = Some(key.get_value()));
    let record = move |_| {
        cx.ui
            .update(|ui| ui.feedback.focused = Some(key.get_value()));
        cx.record_feedback();
    };
    let keyboard = move |event| cx.composer_key(event, true);
    let change = move |_| {
        cx.change(|ui| {
            if let Some(f) = ui.feedback.drafts.get_mut(&key.get_value()) {
                f.category = FeedbackCategory::Change;
            }
        })
    };
    let question = move |_| {
        cx.change(|ui| {
            if let Some(f) = ui.feedback.drafts.get_mut(&key.get_value()) {
                f.category = FeedbackCategory::Question;
            }
        })
    };
    view! {
        <article id=format!("feedback-{id}") class="feedback-card" class:focused=focused class:draft=editing tabindex="-1" on:focus=activate>
            <small>{move || current.get().and_then(|f| f.reference).map(|r| r.label())}</small>
            <Show when=editing fallback=move || view! {
                <span class="category-badge">{move || current.get().map(|f| f.category.label())}</span>
                <p>{move || current.get().map(|f| f.text)}</p>
                <div><button on:click=edit disabled=move || cx.disabled()>"Edit"</button><button on:click=delete disabled=move || cx.disabled()>"Delete"</button></div>
            }>
                <div class="category-toggle" role="radiogroup" aria-label="Feedback category">
                    <button role="radio" aria-checked=move || current.get().is_some_and(|f| f.category == FeedbackCategory::Change).to_string() on:click=change disabled=move || cx.disabled()>"Change"</button>
                    <button role="radio" aria-checked=move || current.get().is_some_and(|f| f.category == FeedbackCategory::Question).to_string() on:click=question disabled=move || cx.disabled()>"Question"</button>
                </div>
                <textarea id=format!("feedback-input-{}", key.get_value()) aria-label="Feedback comment" placeholder="Write your feedback…" prop:value=move || current.get().map(|f| f.text).unwrap_or_default() on:input=draft on:focus=activate on:keydown=keyboard disabled=move || cx.disabled()/>
                <div class="comment-actions"><button class="primary" on:click=record disabled=move || cx.disabled() || current.get().is_none_or(|f| f.text.trim().is_empty())>"Record feedback"</button><button on:click=cancel disabled=move || cx.disabled()>"Cancel"</button></div>
                <span class="draft-hint">"Draft · Enter to record · Shift+Enter for a new line"</span>
            </Show>
        </article>
    }
}

#[component]
pub(crate) fn OverallFeedback(cx: Context) -> impl IntoView {
    let toggle = move |_| cx.change(|ui| ui.feedback.overall_open = !ui.feedback.overall_open);
    let draft = move |event| cx.change(|ui| ui.feedback.overall = event_target_value(&event));
    let submit = move |_| cx.send(Command::Conclude);
    let open = move |_| {
        cx.change(|ui| ui.feedback.overall_open = true);
        focus("overall-feedback");
    };
    view! {
        <section class="overall-drawer" aria-label="Review submission">
            <div class="drawer-header">
                <button class="drawer-toggle" on:click=toggle aria-expanded=move || cx.ui.get().feedback.overall_open.to_string() aria-controls="overall-content">{move || if cx.ui.get().feedback.overall_open { "▾ Overall feedback" } else { "▸ Overall feedback" }}</button>
                <span class="feedback-summary">{move || {
                    let feedback = cx.review.get().map(|r| r.feedback).unwrap_or_default();
                    let changes = feedback.iter().filter(|f| f.category == FeedbackCategory::Change).count();
                    let questions = feedback.len() - changes;
                    format!("{changes} {} · {questions} {}", if changes == 1 { "change" } else { "changes" }, if questions == 1 { "question" } else { "questions" })
                }}</span>
                <span class="draft-summary">{move || {
                    let n = cx.ui.get().feedback.drafts.values().filter(|f| !f.text.trim().is_empty()).count();
                    (n > 0).then(|| format!("{n} inline drafts not included"))
                }}</span>
                <button class="primary" on:click=submit disabled=move || cx.disabled() || cx.review.get().is_some_and(|r| r.busy)>"Submit review"</button>
            </div>
            <Show when=move || cx.ui.get().feedback.overall_open>
                <div id="overall-content" class="overall-content">
                    <textarea id="overall-feedback" aria-label="Overall feedback" placeholder="Your overall assessment (optional)…" prop:value=move || cx.ui.get().feedback.overall on:input=draft disabled=move || cx.disabled()/>
                    <span class="muted">"Saved as you type · Included when you submit"</span>
                </div>
            </Show>
            <Show when=move || !cx.ui.get().feedback.overall_open && !cx.ui.get().feedback.overall.is_empty()>
                <button class="overall-preview" on:click=open>{move || cx.ui.get().feedback.overall}</button>
            </Show>
        </section>
    }
}
