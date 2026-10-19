use crate::{
    chat::Conversation,
    context::Context,
    document_view::DocumentView,
    dom::{document, save_detail},
    feedback::FeedbackPanel,
    prompts::AgentPrompt,
    selection::highlight,
    transport::connect,
};
use leptos::prelude::*;
use review_core::*;
use std::{cell::RefCell, rc::Rc};

#[component]
pub(crate) fn App() -> impl IntoView {
    let cx = Context {
        review: RwSignal::new(None),
        ui: RwSignal::new(UiState::default()),
        connected: RwSignal::new(false),
        error: RwSignal::new(String::new()),
        socket: StoredValue::new_local(Rc::new(RefCell::new(None))),
        pending: StoredValue::new_local(Rc::new(RefCell::new(vec![]))),
    };
    connect(cx);
    let selection = Memo::new(move |_| (cx.ui.get().selection, cx.ui.get().prose_offset));
    let guide = Memo::new(move |_| cx.review.get().map(|r| r.guide_html).unwrap_or_default());
    Effect::new(move || {
        let _ = selection.get();
        let _ = guide.get();
        gloo_timers::callback::Timeout::new(0, move || highlight(cx)).forget();
    });
    let active_tab = Memo::new(move |_| cx.ui.get().tab);
    Effect::new(move |_| {
        let tab = active_tab.get();
        gloo_timers::callback::Timeout::new(0, move || {
            let ui = cx.ui.get_untracked();
            let (id, scroll) = if tab == "feedback" {
                ("feedback-panel", ui.feedback_scroll)
            } else {
                ("conversation", ui.chat_scroll)
            };
            if let Some(el) = document().get_element_by_id(id) {
                el.set_scroll_top(scroll as i32);
            }
        })
        .forget();
    });

    let submit = move |_| cx.send(Command::Conclude);
    let dismiss = move |_| cx.error.set(String::new());
    let show_chat = move |_| cx.change(|ui| ui.tab = "chat".into());
    let show_feedback = move |_| cx.change(|ui| ui.tab = "feedback".into());
    let toggle_activity = move |event| save_detail(cx, "activity", event);
    view! {
        <header class="topbar">
            <div class="brand"><span class="brand-mark">"D"</span><strong>"DevDeck"</strong><span class="muted">"/ Review"</span></div>
            <div class="top-actions">
                <span class="connection" class:offline=move || !cx.connected.get()>{move || {
                    if cx.review.get().is_some_and(|r| r.status == ReviewStatus::Completed) { "Submitted" }
                    else if cx.connected.get() { "Connected" } else { "Reconnecting…" }
                }}</span>
                <button class="primary" on:click=submit disabled=move || cx.disabled() || cx.review.get().is_some_and(|r| r.busy)>"Submit review"</button>
            </div>
        </header>
        {move || cx.review.get().and_then(|r| r.export_path).map(|path| view! {
            <div class="completion" role="status"><strong>"Review submitted. Feedback saved to "</strong><code>{path}</code><p>"The review command has finished. You can close this tab."</p></div>
        })}
        <Show when=move || !cx.error.get().is_empty()>
            <div class="banner error" role="alert">{move || cx.error.get()}<button on:click=dismiss>"Dismiss"</button></div>
        </Show>
        <Show when=move || cx.review.get().is_some() fallback=|| view! { <div class="loading">"Connecting to your review…"</div> }>
            <div class="workspace">
                <DocumentView cx=cx/>
                <aside>
                    <Show when=move || cx.review.get().is_some_and(|r| !r.requests.is_empty())>
                        <section class="pending-requests" aria-label="Pending Codex requests" aria-live="polite">
                            <For each=move || cx.review.get().map(|r| r.requests).unwrap_or_default()
                                key=|request| request.id.to_string() children=move |request| view! { <AgentPrompt cx=cx request=request/> }/>
                        </section>
                    </Show>
                    <nav class="tabs">
                        <button class:active=move || cx.ui.get().tab != "feedback" on:click=show_chat>"Conversation"</button>
                        <button class:active=move || cx.ui.get().tab == "feedback" on:click=show_feedback>{move || format!("Feedback ({})", cx.review.get().map(|r| r.feedback.len()).unwrap_or(0))}</button>
                    </nav>
                    <Show when=move || cx.ui.get().tab != "feedback"><Conversation cx=cx/></Show>
                    <Show when=move || cx.ui.get().tab == "feedback"><FeedbackPanel cx=cx/></Show>
                    <div class="agent-controls">
                        <details open=move || cx.ui.get().details.get("activity").copied().unwrap_or(false) on:toggle=toggle_activity>
                            <summary>"Codex activity"</summary>
                            {move || cx.review.get().map(|r| r.activity.iter().rev().take(20).map(|text| view! { <pre>{text.clone()}</pre> }).collect_view())}
                        </details>
                    </div>
                </aside>
            </div>
        </Show>
    }
}
