use crate::{
    context::Context,
    dom::document,
    reference::{ActiveReference, FileRange},
};
use leptos::prelude::*;
use review_core::*;

#[component]
pub(crate) fn Conversation(cx: Context) -> impl IntoView {
    let scroll = move |_| {
        if let Some(element) = document().get_element_by_id("conversation") {
            cx.change(|ui| ui.chat_scroll = element.scroll_top() as f64);
        }
    };
    let draft = move |event| cx.change(|ui| ui.chat_draft = event_target_value(&event));
    let key = move |event| cx.composer_key(event, false);
    let send = move |_| cx.send_chat();
    let stop = move |_| cx.send(Command::Interrupt);
    view! {
        <div id="conversation" class="conversation" on:scroll=scroll>
            <Show when=move || cx.review.get().is_some_and(|r| r.messages.is_empty())>
                <div class="chat-empty"><h3>"Ask about the change"</h3><p>"Select a passage or a code range, or start with a broader question. Discussion stays separate from feedback."</p></div>
            </Show>
            <For each=move || cx.review.get().map(|r| r.messages).unwrap_or_default() key=|message| message.id.clone()
                children=move |message| view! { <ChatMessage cx=cx message=message/> }/>
        </div>
        <div class="composer">
            <ActiveReference cx=cx/>
            <FileRange cx=cx/>
            <textarea aria-label="Ask Codex" placeholder="Ask Codex about this change…" prop:value=move || cx.ui.get().chat_draft
                on:input=draft on:keydown=key disabled=move || cx.disabled()></textarea>
            <div class="composer-actions">
                <span class="muted">"Enter to send · Shift+Enter for a new line"</span>
                <Show when=move || cx.review.get().is_some_and(|r| r.busy) fallback=move || view! {
                    <button class="primary" disabled=move || cx.disabled() on:click=send>"Send"</button>
                }><button on:click=stop>"Stop"</button></Show>
            </div>
        </div>
    }
}

#[component]
fn ChatMessage(cx: Context, message: Message) -> impl IntoView {
    let id = message.id.clone();
    let current = Memo::new(move |_| {
        cx.review
            .get()
            .and_then(|r| r.messages.into_iter().find(|m| m.id == id))
    });
    let quote = message.reference.as_ref().and_then(|reference| {
        cx.review
            .get_untracked()
            .map(|r| reference.quote(&r.workspace))
    });
    let label = message.reference.as_ref().map(|reference| match reference {
        Reference::Prose { .. } => "Guide selection".to_string(),
        Reference::Code { .. } => reference.label(),
    });
    view! {
        <article class="message" class:user=message.role == MessageRole::User>
            <div class="message-role">{match message.role { MessageRole::User => "You", MessageRole::Assistant => "Codex" }}</div>
            {label.map(|label| view! { <blockquote class="message-reference"><small>{label}</small><pre>{quote}</pre></blockquote> })}
            <div class="message-body" inner_html=move || current.get().map(|m| document::markdown(&m.text)).unwrap_or_default()></div>
            <small class="muted">{move || current.get().filter(|m| m.status != MessageStatus::Complete).map(|m| m.status.to_string()).unwrap_or_default()}</small>
        </article>
    }
}
