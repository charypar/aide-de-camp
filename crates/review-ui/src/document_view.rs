use crate::{
    context::Context,
    dom::{document, save_detail},
    selection::SelectionController,
};
use leptos::prelude::*;
use review_core::*;

#[component]
pub(crate) fn DocumentView(cx: Context) -> impl IntoView {
    let guide = Memo::new(move |_| cx.review.get().map(|r| r.guide_html).unwrap_or_default());
    let selection = SelectionController::new(cx);
    let pointer_down = move |event| selection.pointer_down(event);
    let pointer_move = move |event| selection.pointer_move(event);
    let pointer_up = move |event| selection.pointer_up(event);
    let drag_release = window_event_listener(leptos::ev::mouseup, move |_| selection.finish_drag());
    on_cleanup(move || drag_release.remove());
    let reading_scroll = move |_| {
        if let Some(element) = document().get_element_by_id("reading") {
            cx.change(|ui| ui.scroll = element.scroll_top() as f64);
        }
    };
    let retry = move |_| cx.send(Command::Generate);
    let toggle_changes = move |event| save_detail(cx, "all-changes", event);
    view! {
        <main id="reading" class:has-title=move || guide.get().contains("<h1>") on:scroll=reading_scroll>
            <div class="document-heading">
                <p class="muted directory">{move || cx.review.get().map(|r| r.directory).unwrap_or_default()}</p>
                <div class="document-meta">{move || cx.review.get().map(|r| format!("{} · {} changed files · {}", r.workspace.vcs, r.workspace.files.len(), r.status)).unwrap_or_default()}</div>
            </div>
            {move || cx.review.get().and_then(|r| r.notice).map(|text| view! { <div class="banner">{text}</div> })}
            {move || cx.review.get().and_then(|r| r.error).map(|text| view! { <div class="banner error">{text}</div> })}
            <Show when=move || cx.review.get().is_some_and(|r| r.generating)>
                <div class="guide-loading" role="status">
                    <p>{move || if cx.review.get().is_some_and(|r| !r.requests.is_empty()) { "Preparation is waiting for your response to the Codex request." } else { "Codex is preparing the walkthrough…" }}</p>
                    <PreparationClock cx=cx/>
                    <div class="skeleton heading"></div><div class="skeleton"></div><div class="skeleton"></div><div class="skeleton code"></div>
                </div>
            </Show>
            <div id="guide" on:mousedown=pointer_down on:mousemove=pointer_move on:mouseup=pointer_up inner_html=move || guide.get()></div>
            {move || cx.review.get().map(|r| r.guide_warnings.into_iter().map(|warning| view! { <p class="warning">{warning}</p> }).collect_view())}
            <Show when=move || cx.review.get().is_some_and(|r| r.guide.is_empty() && !r.busy && r.status != ReviewStatus::Completed)>
                <button disabled=move || cx.disabled() on:click=retry>"Retry preparation"</button>
            </Show>
            <details class="all-changes" open=move || cx.ui.get().details.get("all-changes").copied().unwrap_or(false) on:toggle=toggle_changes>
                <summary>"All reviewed changes"</summary>
                <p class="muted">"Use the source reference fields in the conversation to discuss any range, including code outside the guide."</p>
                <pre>{move || cx.review.get().map(|r| r.workspace.diff).unwrap_or_default()}</pre>
            </details>
        </main>
    }
}

#[component]
fn PreparationClock(cx: Context) -> impl IntoView {
    let now = RwSignal::new(js_sys::Date::now() as u64);
    let timer = StoredValue::new_local(Some(gloo_timers::callback::Interval::new(
        1000,
        move || {
            now.set(js_sys::Date::now() as u64);
        },
    )));
    on_cleanup(move || {
        timer.update_value(|timer| {
            timer.take();
        })
    });

    view! {
        <p class="preparation-clock muted" role="timer" aria-live="off">
            {move || cx.review.get().and_then(|review| review.preparation_elapsed(now.get())).map(|elapsed| format!("Elapsed {elapsed}"))}
        </p>
    }
}
