use crate::{configuration::Section, transport::Context};
use aide_core::{Lifecycle, Status};
use leptos::prelude::*;

#[component]
pub fn Header(section: RwSignal<Section>, machine: RwSignal<String>) -> impl IntoView {
    let cx = expect_context::<Context>();
    view! {
        <header class="topbar">
            <div class="brand"><strong>"Aide de camp"</strong>
                <nav class="section-nav" aria-label="Sections">
                    <a href="#/tasks" aria-current=move || (section.get() == Section::Tasks).then_some("page")>"Tasks"</a>
                </nav>
            </div>
            <div class="header-controls">
                <nav class="machines" aria-label="Machines">
                    <Machine instance="local".to_owned() machine=machine/>
                    <For each={move || cx.hosts.get().into_iter().map(|host| host.identity.instance).collect::<Vec<_>>()} key=|id| id.clone() children={move |instance| view! { <Machine instance=instance machine=machine/> }}/>
                </nav>
                <a class="configuration-link" href="#/configuration" aria-label="AI configuration" title="AI configuration" aria-current=move || matches!(section.get(), Section::Configuration(_)).then_some("page")>
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" aria-hidden="true" focusable="false">
                        <polygon points="21.45,11.07 21.45,12.93 19.08,14.15 18.53,15.49 19.34,18.03 18.03,19.34 15.49,18.53 14.15,19.08 12.93,21.45 11.07,21.45 9.85,19.08 8.51,18.53 5.97,19.34 4.66,18.03 5.47,15.49 4.92,14.15 2.55,12.93 2.55,11.07 4.92,9.85 5.47,8.51 4.66,5.97 5.97,4.66 8.51,5.47 9.85,4.92 11.07,2.55 12.93,2.55 14.15,4.92 15.49,5.47 18.03,4.66 19.34,5.97 18.53,8.51 19.08,9.85"/>
                        <circle cx="12" cy="12" r="3"/>
                    </svg>
                </a>
            </div>
        </header>
    }
}

#[component]
fn Machine(instance: String, machine: RwSignal<String>) -> impl IntoView {
    let cx = expect_context::<Context>();
    let instance = StoredValue::new(instance);
    let summary = Memo::new(move |_| {
        let id = instance.get_value();
        let (name, online) = if id == "local" {
            (
                cx.local
                    .get()
                    .map(|host| host.name)
                    .unwrap_or_else(|| "This machine".into()),
                cx.connected.get(),
            )
        } else {
            cx.hosts
                .get()
                .into_iter()
                .find(|host| host.identity.instance == id)
                .map(|host| {
                    (
                        host.identity.name,
                        cx.connected.get() && host.error.is_none(),
                    )
                })
                .unwrap_or_else(|| (id.clone(), false))
        };
        let mut attention = 0;
        let mut ready = 0;
        for project in cx.projects.get().iter().filter(|p| {
            p.source
                .as_ref()
                .map(|s| s.instance.as_str())
                .unwrap_or("local")
                == id
        }) {
            for task in &project.tasks {
                if task.task.lifecycle != Lifecycle::Completed {
                    match task.status {
                        Status::Attention => attention += 1,
                        Status::Waiting => ready += 1,
                        _ => {}
                    }
                }
            }
        }
        (name, online, attention, ready)
    });
    let select = move |_| {
        machine.update(|selected| {
            *selected = if *selected == instance.get_value() {
                String::new()
            } else {
                instance.get_value()
            };
        });
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_hash("/tasks");
        }
    };
    view! {
        <button class="machine" data-machine=instance.get_value()
            class:offline=move || !summary.get().1
            aria-pressed=move || (machine.get() == instance.get_value()).to_string()
            title=move || { let (name, online, attention, ready) = summary.get();
                if online { format!("{name}: Connected · {attention} attention · {ready} ready. Click to filter; click again for all machines.") }
                else { format!("{name}: Disconnected") }
            }
            on:click=select>
            <span class="machine-name">{move || summary.get().0}</span>
            <span class="machine-counts">{move || { let (_, online, attention, ready) = summary.get(); if online { format!("({attention}.{ready})") } else { "—".into() } }}</span>
            <span class="connection-dot" aria-hidden="true"></span>
        </button>
    }
}
