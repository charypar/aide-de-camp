use crate::transport::Context;
use aide_core::{Command, Lifecycle, PrView, TaskView};
use leptos::prelude::*;

#[derive(Clone, Copy)]
pub struct TaskUi {
    pub expanded: RwSignal<bool>,
    pub linking: RwSignal<bool>,
    pub draft: RwSignal<String>,
    pub request: RwSignal<String>,
    pub adding: RwSignal<bool>,
    pub message: RwSignal<String>,
    pub error: RwSignal<bool>,
}
impl TaskUi {
    pub fn new() -> Self {
        Self {
            expanded: RwSignal::new(false),
            linking: RwSignal::new(false),
            draft: RwSignal::new(String::new()),
            request: RwSignal::new(String::new()),
            adding: RwSignal::new(false),
            message: RwSignal::new(String::new()),
            error: RwSignal::new(false),
        }
    }
    fn begin(self, cx: Context, adding: bool) -> String {
        let id = cx.next_request();
        self.request.set(id.clone());
        self.adding.set(adding);
        self.error.set(false);
        self.message.set("Checking GitHub…".into());
        id
    }
}
fn description(pr: &PrView) -> String {
    let mut parts = vec![pr.id.label(), pr.label()];
    if let Some(o) = &pr.observation {
        parts.extend([
            o.title.clone(),
            format!(
                "{} · @{} · signed in as @{}",
                o.role.label(),
                o.author,
                o.viewer
            ),
            o.reasons.join(", "),
        ]);
    }
    if let Some(error) = &pr.error {
        parts.push(error.clone());
    }
    parts.join(" · ")
}
#[component]
pub fn PrTags(task: Memo<Option<TaskView>>) -> impl IntoView {
    view! { <div class="pr-tags">{move || task.get().map(|t| t.pull_requests.into_iter().map(|pr| {
        view! { <a class="pr-tag" data-color=pr.color() href=pr.id.url() target="_blank" rel="noopener noreferrer" title=description(&pr)>{format!("{} · {}",pr.id.label(),pr.label())}</a> }
    }).collect_view())}</div> }
}
#[component]
pub fn PrDetails(
    task: Memo<Option<TaskView>>,
    root: StoredValue<String>,
    id: StoredValue<String>,
    ui: TaskUi,
) -> impl IntoView {
    let cx = expect_context::<Context>();
    let editable = move || {
        task.get()
            .is_some_and(|t| t.task.lifecycle == Lifecycle::Open)
    };
    let disabled = move || !cx.connected.get() || !ui.request.get().is_empty();
    let open_link = move |_| {
        ui.linking.set(true);
        ui.message.set(String::new());
    };
    let cancel = move |_| {
        ui.linking.set(false);
        ui.draft.set(String::new());
        ui.message.set(String::new());
    };
    let change = move |event| ui.draft.set(event_target_value(&event));
    let add = move |event: web_sys::SubmitEvent| {
        event.prevent_default();
        if disabled() {
            return;
        }
        let request_id = ui.begin(cx, true);
        cx.send(Command::LinkPr {
            project: root.get_value(),
            id: id.get_value(),
            url: ui.draft.get_untracked(),
            request_id,
        });
    };
    let refresh = move |_| {
        let request_id = ui.begin(cx, false);
        cx.send(Command::RefreshPrs {
            project: root.get_value(),
            id: id.get_value(),
            request_id,
        });
    };
    view! {
        <section class="github-prs" aria-label="GitHub PRs">
            <div class="pr-section-heading"><strong>"GitHub PRs"</strong><Show when=editable><button on:click=open_link disabled=disabled>"Link PR"</button><Show when=move || task.get().is_some_and(|t| !t.pull_requests.is_empty())><button on:click=refresh disabled=disabled>"Refresh PRs"</button></Show></Show></div>
            <Show when=move || task.get().is_none_or(|t| t.pull_requests.is_empty())><p class="empty-line">"No linked PRs"</p></Show>
            <Show when=move || editable() && ui.linking.get()><form class="pr-link-form" on:submit=add>
                <label>"PR URL"<input type="url" required placeholder="https://github.com/owner/repo/pull/123" prop:value=move || ui.draft.get() on:input=change disabled=disabled/></label>
                <button type="submit" class="primary" disabled=disabled>"Add"</button><button type="button" on:click=cancel disabled=disabled>"Cancel"</button>
            </form></Show>
            <p class="pr-message" class:error=move || ui.error.get() aria-live="polite">{move || ui.message.get()}</p>
            {move || task.get().map(|t| t.pull_requests.into_iter().map(|pr| {
                let url = pr.id.url();
                let unlink = move |_| {
                    let request_id = ui.begin(cx,false);
                    cx.send(Command::UnlinkPr {project:root.get_value(),id:id.get_value(),url:url.clone(),request_id});
                };
                let unlink_label = format!("Unlink {}",pr.id.label());
                let label = pr.label();
                let description = description(&pr);
                let title = pr.observation.as_ref().map(|o| o.title.clone()).unwrap_or_else(||pr.id.label());
                view! {<article class="pr-entry">
                    <div class="pr-entry-heading"><a href=pr.id.url() target="_blank" rel="noopener noreferrer">{format!("{} · {title}",pr.id.label())}</a><Show when=editable><button on:click=unlink.clone() disabled=disabled aria-label=unlink_label.clone()>"Unlink"</button></Show></div>
                    <p class="pr-description" title=description><span class="pr-tag" data-color=pr.color()>{label}</span>{pr.observation.as_ref().map(|o|format!(" {} · @{} · signed in as @{}",o.role.label(),o.author,o.viewer))}</p>
                    {pr.observation.as_ref().map(|o|view!{<p class="pr-reasons">{o.reasons.join(" · ")}</p>})}
                    <p class="error">{pr.error.clone()}</p>
                    {pr.observation.as_ref().map(|o|view!{
                        {o.review.map(|review|view!{<p class="pr-review">{format!("Review: {}",review.label())}</p>})}
                        <p class="pr-freshness">{format!("{}: {}",if pr.historical {"Historical observation"} else if pr.stale {"Last known status"} else {"Checked"}, timestamp(o.observed_at))}</p>
                        {o.head.as_ref().map(|head|view!{<p class="pr-context">{format!("Published: {} → {} · {}",head,o.base.as_deref().unwrap_or(""),o.head_sha.as_deref().unwrap_or(""))}</p>})}
                        {(!o.checks.is_empty()).then(||view!{<details class="pr-checks"><summary>"CI checks"</summary><ul>{o.checks.iter().map(|check| {
                            let text = format!("{}: {}{}",check.name,check.result,if check.required {" (required)"} else {""});
                            view!{<li>{match &check.url {Some(url)=>view!{<a href=url.clone() target="_blank" rel="noopener noreferrer">{text}</a>}.into_any(),None=>text.into_any()}}</li>}
                        }).collect_view()}</ul></details>})}
                    })}
                </article>}
            }).collect_view())}
        </section>
    }
}
fn timestamp(seconds: u64) -> String {
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds as f64 * 1000.0))
        .to_locale_string("en-GB", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_else(|| seconds.to_string())
}
