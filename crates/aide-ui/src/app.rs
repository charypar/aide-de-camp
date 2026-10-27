use crate::transport::Context;
use aide_core::{Command, Lifecycle, ProjectView, Status};
use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    let cx = Context::new();
    provide_context(cx);
    cx.connect();
    let focus = RwSignal::new(String::new());
    let completed = RwSignal::new(false);
    Effect::new(move |_| {
        let projects = cx.projects.get();
        let mut attention = 0;
        let mut ready = 0;
        for task in projects.iter().flat_map(|project| &project.tasks) {
            if task.task.lifecycle == Lifecycle::Completed {
                continue;
            }
            match task.status {
                Status::Attention => attention += 1,
                Status::Waiting => ready += 1,
                _ => {}
            }
        }
        let title = if attention == 0 && ready == 0 {
            "Aide de camp".to_owned()
        } else {
            format!("({attention}·{ready}) Aide de camp")
        };
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            document.set_title(&title);
        }
    });
    let show_all = move |_| focus.set(String::new());
    let toggle_history = move |_| completed.update(|v| *v = !*v);
    view! {
        <header class="topbar"><div class="brand"><strong>"Aide de camp"</strong><span class="subtitle">"Tasks"</span></div>
            <span class="connection" class:offline=move || !cx.connected.get()>{move || if cx.connected.get() { "Connected" } else { "Reconnecting…" }}</span>
        </header>
        <main>
            <nav aria-label="Project navigation"><button class="project-filter" aria-pressed=move || focus.get().is_empty().to_string() on:click=show_all>"All projects"</button><label class="history"><input type="checkbox" on:change=toggle_history prop:checked=move || completed.get()/>"Show completed"</label></nav>
            <p class="notice" class:error=move || cx.error.get() role="status">{move || cx.message.get()}</p>
            <Show when=move || cx.projects.get().is_empty()><section class="empty"><h1>"Make room for your next task"</h1><p>"Register a project from your terminal:"</p><code>"aide init . --repo ./repository"</code></section></Show>
            <For each={move || cx.projects.get().into_iter().filter(|p| focus.get().is_empty() || focus.get() == p.root).map(|p| p.root).collect::<Vec<_>>()} key=|root| root.clone() children={move |root| view! { <Project root=root focus=focus completed=completed/> }}/>
        </main>
    }
}

#[component]
fn Project(root: String, focus: RwSignal<String>, completed: RwSignal<bool>) -> impl IntoView {
    let cx = expect_context::<Context>();
    let root = StoredValue::new(root);
    let project = Memo::new(move |_| {
        cx.projects
            .get()
            .into_iter()
            .find(|p| p.root == root.get_value())
    });
    let creating = RwSignal::new(false);
    let id = RwSignal::new(String::new());
    let repository = RwSignal::new(String::new());
    let selected_repository = Memo::new(move |_| {
        let repositories = project.get().map(|p| p.repositories).unwrap_or_default();
        let selected = repository.get();
        repositories
            .iter()
            .find(|r| r.name == selected)
            .or_else(|| repositories.first())
            .map(|r| r.name.clone())
            .unwrap_or_default()
    });
    let focus_project = move |_| focus.set(root.get_value());
    let toggle_create = move |_| creating.update(|v| *v = !*v);
    let change_id = move |event| id.set(event_target_value(&event));
    let change_repository = move |event| repository.set(event_target_value(&event));
    let start_task = move |event: web_sys::SubmitEvent| {
        event.prevent_default();
        let repo = selected_repository.get();
        cx.send(Command::Start {
            project: root.get_value(),
            id: id.get().trim().into(),
            repository: if repo.is_empty() { None } else { Some(repo) },
            open_terminal: true,
        });
    };
    view! {
        <section class="project">
            <div class="project-heading"><button class="project-name" on:click=focus_project>{move || project.get().map(|p| p.name).unwrap_or_default()}</button><button class="primary" on:click=toggle_create aria-expanded=move || creating.get().to_string() disabled=move || !cx.connected.get()>"New task"</button></div>
            <p class="path">{root.get_value()}</p>
            <Show when=move || creating.get()><form class="create" on:submit=start_task>
                <label>"Task ID"<input required pattern=r"[A-Za-z0-9][A-Za-z0-9_\-]{0,79}" placeholder="task-1234" on:input=change_id prop:value=move || id.get()/></label>
                <label>"Repository"<select required on:change=change_repository>
                    <Show when=move || selected_repository.get().is_empty()><option value="">"No repositories available"</option></Show>
                    {move || project.get().map(|p| p.repositories.into_iter().map(|r| {
                        let name = r.name.clone();
                        view! { <option value=r.name.clone() prop:selected=move || selected_repository.get() == name>{r.name.clone()}</option> }
                    }).collect_view())}
                </select></label>
                <button class="primary" type="submit" disabled=move || !cx.connected.get() || selected_repository.get().is_empty()>"Start and open Codex"</button>
            </form></Show>
            <p class="error">{move || project.get().and_then(|p| p.error)}</p>
            { [Status::Attention, Status::Waiting, Status::Working, Status::Completed].into_iter().map(move |status| {
                let group = status.clone();
                let list = Memo::new(move |_| project.get().map(|p| p.tasks.into_iter().filter(|t| t.status == group && (completed.get() || t.task.lifecycle != Lifecycle::Completed)).map(|t| t.task.id).collect::<Vec<_>>()).unwrap_or_default());
                view! { <Show when=move || !list.get().is_empty()><h3>{label(&status)}</h3><For each=move || list.get() key=|id| id.clone() children={move |id| view!{<Task root=root.get_value() id=id/>}}/></Show> }
            }).collect_view() }
            <Show when=move || project.get().is_some_and(|p| p.tasks.iter().all(|t| t.task.lifecycle == Lifecycle::Completed))><p class="empty-line">"No active tasks. Start one when you're ready."</p></Show>
        </section>
    }
}

#[component]
fn Task(root: String, id: String) -> impl IntoView {
    let cx = expect_context::<Context>();
    let root = StoredValue::new(root);
    let id = StoredValue::new(id);
    let task = Memo::new(move |_| {
        cx.projects
            .get()
            .into_iter()
            .find(|p: &ProjectView| p.root == root.get_value())
            .and_then(|p| p.tasks.into_iter().find(|t| t.task.id == id.get_value()))
    });
    let expanded = RwSignal::new(false);
    let summary = Memo::new(move |_| {
        task.get()
            .and_then(|t| t.summary)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Untitled task".into())
    });
    let toggle = move |_| expanded.update(|v| *v = !*v);
    let attach = move |_| {
        cx.send(Command::Attach {
            project: root.get_value(),
            id: id.get_value(),
        })
    };
    let recover = move |_| {
        cx.send(Command::Recover {
            environment: None,
            project: root.get_value(),
            id: id.get_value(),
        })
    };
    let editor = move |_| {
        cx.send(Command::Editor {
            project: root.get_value(),
            id: id.get_value(),
        })
    };
    let review = move |_| {
        cx.send(Command::Review {
            project: root.get_value(),
            id: id.get_value(),
        })
    };
    let cleanup = move |_| {
        if let Some(task) = task.get() {
            let warning = if task.backend_alive || task.terminal_alive {
                format!(
                    "\nA Codex session or task terminal is open. {}",
                    task.reason
                )
            } else {
                String::new()
            };
            let message = format!(
                "Close {} and delete its workspace?{}\n{}\nHistory and recordings will be retained.",
                id.get_value(),
                warning,
                task.task.workspace
            );
            if web_sys::window()
                .unwrap()
                .confirm_with_message(&message)
                .unwrap_or(false)
            {
                cx.send(Command::Cleanup {
                    project: root.get_value(),
                    id: id.get_value(),
                    confirmed: true,
                });
            }
        }
    };
    let copy_path = move |_| {
        if let Some(task) = task.get() {
            let _ = web_sys::window()
                .unwrap()
                .navigator()
                .clipboard()
                .write_text(&task.task.workspace);
        }
    };
    let is_open = move || {
        task.get()
            .is_some_and(|t| t.task.lifecycle != Lifecycle::Completed)
    };
    view! {
        <article class="task">
            <div class="task-heading"><button class="task-title" on:click=toggle aria-expanded=move || expanded.get().to_string()>
                <span class="chevron" aria-hidden="true">{move || if expanded.get() { "▾" } else { "▸" }}</span>
                <span class="task-summary" title=move || summary.get()>{move || summary.get()}</span>
                <span class="task-id" title=id.get_value()>{id.get_value()}</span>
                <span class="task-status" data-status=move || task.get().map(|t| status_name(&t.status)) title=move || task.get().map(|t| t.reason)>
                    {move || task.get().map(|t| label(&t.status))}
                </span>
            </button>
                <Show when=is_open><button class="terminal-action" aria-label="Open terminal" title="Open terminal" on:click=attach disabled=move || !cx.connected.get()>
                    <span class="terminal-label">"Open terminal"</span><span class="terminal-icon" aria-hidden="true">">_"</span>
                </button></Show>
            </div>
            <Show when=move || expanded.get()>
                <div class="details">
                    <p class="expanded-summary">{move || summary.get()}</p>
                    <p class="task-context"><span class="repo">{move || task.get().map(|t| t.task.repository)}</span><span class="reason">{move || task.get().map(|t| t.reason)}</span></p>
                    <code>{move || task.get().map(|t| t.task.workspace)}</code><button on:click=copy_path>"Copy path"</button>
                    <dl><dt>"Conversation"</dt><dd>{move || task.get().and_then(|t| t.task.thread_id).unwrap_or_else(|| "Not started".into())}</dd><dt>"Recording"</dt><dd>{move || task.get().and_then(|t| t.task.recording).unwrap_or_else(|| "Not recorded yet".into())}</dd></dl>
                    <Show when=is_open><div class="actions"><button on:click=editor>"Open Zed"</button><button on:click=review>"Review"</button><button on:click=recover>"Recover session"</button><button class="danger" on:click=cleanup>"Close task"</button></div></Show>
                </div>
            </Show>
        </article>
    }
}

fn status_name(status: &Status) -> &'static str {
    match status {
        Status::Attention => "attention",
        Status::Working => "working",
        Status::Waiting => "waiting",
        Status::Completed => "completed",
    }
}

fn label(status: &Status) -> &'static str {
    match status {
        Status::Attention => "Requires attention",
        Status::Working => "Working",
        Status::Waiting => "Ready",
        Status::Completed => "Completed",
    }
}
