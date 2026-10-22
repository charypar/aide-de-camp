use crate::{
    annotations::highlight, chat::Conversation, context::Context, document_view::DocumentView,
    dom::save_detail, feedback::OverallFeedback, prompts::AgentPrompt, transport::connect,
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
    let anchors = Memo::new(move |_| {
        let ui = cx.ui.get();
        (
            ui.document.selection,
            ui.document.prose_offset,
            ui.document.code_block,
            ui.document.selection_actions,
            ui.chat.collapsed,
            ui.feedback.overall_open,
            ui.feedback.drafts,
            cx.comments(),
        )
    });
    let guide = Memo::new(move |_| cx.review.get().map(|r| r.guide_html).unwrap_or_default());
    Effect::new(move || {
        let _ = anchors.get();
        let _ = guide.get();
        gloo_timers::callback::Timeout::new(0, move || highlight(cx)).forget();
    });
    let resize = window_event_listener(leptos::ev::resize, move |_| crate::annotations::layout(cx));
    on_cleanup(move || resize.remove());

    let dismiss = move |_| cx.error.set(String::new());
    let toggle_chat = move |_| cx.change(|ui| ui.chat.collapsed = !ui.chat.collapsed);
    let chat_label = move || {
        if cx.ui.get().chat.collapsed {
            "Show chat"
        } else {
            "Hide chat"
        }
    };
    let show_requests = move |_| cx.change(|ui| ui.chat.collapsed = false);
    let toggle_activity = move |event| save_detail(cx, "activity", event);
    view! {
        <header class="topbar">
            <div class="brand">
                <button class="chat-toggle" on:click=toggle_chat aria-label=chat_label title=chat_label aria-expanded=move || (!cx.ui.get().chat.collapsed).to_string() aria-controls="chat-panel">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg>
                </button>
                <strong>"DevDeck"</strong><span class="muted">"/ Review"</span>
            </div>
            <div class="top-actions">
                <span class="connection" class:offline=move || !cx.connected.get()>{move || {
                    if cx.review.get().is_some_and(|r| r.status == ReviewStatus::Completed) { "Submitted" }
                    else if cx.connected.get() { "Connected" } else { "Reconnecting…" }
                }}</span>
                <Show when=move || cx.ui.get().chat.collapsed && cx.review.get().is_some_and(|r| !r.requests.is_empty())>
                    <button class="request-indicator" on:click=show_requests>"Codex needs your response"</button>
                </Show>
            </div>
        </header>
        {move || cx.review.get().filter(|r| r.status == ReviewStatus::Completed).map(|review| {
            let message = if review.export_path.is_some() { "Review submitted. Feedback saved to " } else { NO_FEEDBACK_MESSAGE };
            view! {
                <div class="completion" role="status"><strong>{message}</strong>{review.export_path.map(|path| view! { <code>{path}</code> })}<p>"The review command has finished. You can close this tab."</p></div>
            }
        })}
        <Show when=move || !cx.error.get().is_empty()>
            <div class="banner error" role="alert">{move || cx.error.get()}<button on:click=dismiss>"Dismiss"</button></div>
        </Show>
        <Show when=move || cx.review.get().is_some() fallback=|| view! { <div class="loading">"Connecting to your review…"</div> }>
            <div class="workspace" class:chat-collapsed=move || cx.ui.get().chat.collapsed>
                <DocumentView cx=cx/>
                <Show when=move || !cx.ui.get().chat.collapsed>
                <aside id="chat-panel" class="chat-panel" aria-label="Agent conversation">
                    <Show when=move || cx.review.get().is_some_and(|r| !r.requests.is_empty())>
                        <section class="pending-requests" aria-label="Pending Codex requests" aria-live="polite">
                            <For each=move || cx.review.get().map(|r| r.requests).unwrap_or_default()
                                key=|request| request.id.to_string() children=move |request| view! { <AgentPrompt cx=cx request=request/> }/>
                        </section>
                    </Show>
                    <div class="chat-heading">"Conversation"</div>
                    <Conversation cx=cx/>
                    <div class="agent-controls">
                        <details open=move || cx.ui.get().details.get("activity").copied().unwrap_or(false) on:toggle=toggle_activity>
                            <summary>"Codex activity"</summary>
                            {move || cx.review.get().map(|r| r.activity.iter().rev().take(20).map(|text| view! { <pre>{text.clone()}</pre> }).collect_view())}
                        </details>
                    </div>
                </aside>
                </Show>
            </div>
            <OverallFeedback cx=cx/>
        </Show>
    }
}
