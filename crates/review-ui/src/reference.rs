use crate::{context::Context, dom::save_detail};
use leptos::prelude::*;

#[component]
pub(crate) fn ActiveReference(cx: Context) -> impl IntoView {
    let clear = move |_| cx.clear_reference();
    let feedback = move |_| cx.change(|ui| ui.tab = "feedback".into());
    view! {
        <div class="reference">
            <div class="eyebrow">{move || if cx.ui.get().selection.is_some() { "ACTIVE REFERENCE" } else { "WHOLE REVIEW" }}</div>
            <p>{move || cx.ui.get().selection.map(|r| r.label()).unwrap_or_else(|| "Select guide text or code for a specific reference, or leave general feedback.".into())}</p>
            <Show when=move || cx.ui.get().selection.is_some()>
                <button on:click=clear>"Clear reference"</button>
                <Show when=move || cx.ui.get().tab != "feedback"><button on:click=feedback>"Add feedback"</button></Show>
            </Show>
        </div>
    }
}

#[component]
pub(crate) fn FileRange(cx: Context) -> impl IntoView {
    let toggle = move |event| save_detail(cx, "manual", event);
    let file = move |event| cx.change(|ui| ui.manual_file = event_target_value(&event));
    let start = move |event| cx.change(|ui| ui.manual_start = event_target_value(&event));
    let end = move |event| cx.change(|ui| ui.manual_end = event_target_value(&event));
    let side = move |event| {
        cx.change(|ui| ui.manual_side = event_target_value(&event).parse().unwrap_or_default())
    };
    let use_range = move |_| cx.use_file_range();
    view! {
        <details class="manual" open=move || cx.ui.get().details.get("manual").copied().unwrap_or(false) on:toggle=toggle>
            <summary>"Reference a file range"</summary>
            <input aria-label="Source file" placeholder="Repository-relative path" list="changed-files" prop:value=move || cx.ui.get().manual_file on:input=file/>
            <datalist id="changed-files">{move || cx.review.get().map(|r| r.workspace.files.into_iter().map(|file| view! { <option value=file.path/> }).collect_view())}</datalist>
            <div class="range-fields">
                <input aria-label="Start line" placeholder="Start" type="number" min="1" prop:value=move || cx.ui.get().manual_start on:input=start/>
                <input aria-label="End line" placeholder="End" type="number" min="1" prop:value=move || cx.ui.get().manual_end on:input=end/>
                <select aria-label="Diff side" prop:value=move || cx.ui.get().manual_side.as_str() on:change=side><option value="new">"New"</option><option value="old">"Old"</option></select>
                <button on:click=use_range>"Use"</button>
            </div>
        </details>
    }
}
