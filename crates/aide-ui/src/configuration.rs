use crate::transport::{Context, NoticeKind};
use aide_core::{Command, ConfigAction, ConfigData, ConfigDocument, ConfigListing};
use leptos::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};

#[derive(Clone)]
pub enum DocumentState {
    Empty,
    Loading,
    Ready(ConfigDocument),
    Error(String),
}

#[derive(Clone, Copy)]
pub struct ConfigUi {
    pub listing: RwSignal<Option<ConfigListing>>,
    pub list_error: RwSignal<String>,
    pub document: RwSignal<DocumentState>,
    list_request: RwSignal<String>,
    read_request: RwSignal<String>,
    open_request: RwSignal<String>,
}

impl ConfigUi {
    pub fn new() -> Self {
        Self {
            listing: RwSignal::new(None),
            list_error: RwSignal::new(String::new()),
            document: RwSignal::new(DocumentState::Empty),
            list_request: RwSignal::new(String::new()),
            read_request: RwSignal::new(String::new()),
            open_request: RwSignal::new(String::new()),
        }
    }

    pub fn receive(self, cx: Context, request: String, result: Result<ConfigData, String>) {
        if self.list_request.get_untracked() == request {
            self.list_request.set(String::new());
            match result {
                Ok(ConfigData::Listing(listing)) => {
                    self.listing.set(Some(listing));
                    self.list_error.set(String::new());
                }
                Err(error) => self.list_error.set(error),
                _ => {}
            }
        } else if self.read_request.get_untracked() == request {
            self.read_request.set(String::new());
            match result {
                Ok(ConfigData::Document(doc)) => self.document.set(DocumentState::Ready(doc)),
                Err(error) => self.document.set(DocumentState::Error(error)),
                _ => {}
            }
        } else if self.open_request.get_untracked() == request {
            self.open_request.set(String::new());
            match result {
                Ok(ConfigData::Opened) => cx.notify("Opened in Zed".into(), NoticeKind::Success),
                Err(error) => cx.notify(error, NoticeKind::Error),
                _ => {}
            }
        }
    }

    fn request(self, cx: Context, operation: ConfigAction, slot: RwSignal<String>) {
        let request_id = cx.next_request();
        slot.set(request_id.clone());
        if !cx.send(Command::Configuration {
            request_id,
            operation,
        }) {
            slot.set(String::new());
        }
    }

    fn list(self, cx: Context) {
        self.list_error.set(String::new());
        self.request(cx, ConfigAction::List, self.list_request);
    }

    fn read(self, cx: Context, path: String) {
        self.read_request.set(String::new());
        if path.is_empty() {
            self.document.set(DocumentState::Empty);
            return;
        }
        self.document.set(DocumentState::Loading);
        self.request(cx, ConfigAction::Read { path }, self.read_request);
    }

    fn open(self, cx: Context, path: Option<String>, line: u32) {
        self.request(cx, ConfigAction::Open { path, line }, self.open_request);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    Tasks,
    Configuration(String),
}

impl Section {
    fn current() -> Self {
        let hash = web_sys::window()
            .unwrap()
            .location()
            .hash()
            .unwrap_or_default();
        if hash == "#/configuration" {
            return Self::Configuration(String::new());
        }
        if let Some(path) = hash.strip_prefix("#/configuration/") {
            return Self::Configuration(
                js_sys::decode_uri_component(path)
                    .ok()
                    .and_then(|s| s.as_string())
                    .unwrap_or_default(),
            );
        }
        Self::Tasks
    }

    pub fn signal() -> RwSignal<Self> {
        let section = RwSignal::new(Self::current());
        let listener =
            Closure::<dyn FnMut(web_sys::Event)>::new(move |_| section.set(Self::current()));
        let window = web_sys::window().unwrap();
        window
            .add_event_listener_with_callback("hashchange", listener.as_ref().unchecked_ref())
            .unwrap();
        // This single listener lives for the application lifetime.
        listener.forget();
        section
    }

    fn link(path: &str) -> String {
        format!("#/configuration/{}", js_sys::encode_uri_component(path))
    }
}

#[component]
pub fn Configuration(section: RwSignal<Section>) -> impl IntoView {
    let cx = expect_context::<Context>();
    let ui = cx.configuration;
    let desktop = Memo::new(move |_| cx.local.get().is_some_and(|host| host.desktop));
    let filter = RwSignal::new(String::new());
    let line = RwSignal::new(1u32);
    let active = Memo::new(move |_| matches!(section.get(), Section::Configuration(_)));
    let selected = Memo::new(move |_| match section.get() {
        Section::Configuration(path) => path,
        _ => String::new(),
    });

    Effect::new(move |_| {
        if active.get() && cx.connected.get() {
            ui.list(cx);
        }
    });
    Effect::new(move |_| {
        let path = selected.get();
        line.set(1);
        if active.get() && cx.connected.get() {
            ui.read(cx, path);
        }
    });
    let refresh = move |_| {
        ui.list(cx);
        ui.read(cx, selected.get_untracked());
        line.set(1);
    };
    let change_filter = move |event| filter.set(event_target_value(&event));
    let open_folder = move |_| ui.open(cx, None, 1);
    let open_file = move |_| ui.open(cx, Some(selected.get_untracked()), line.get_untracked());
    let copy_reference = move |_| {
        if let Some(listing) = ui.listing.get_untracked() {
            let reference = if selected.get_untracked().is_empty() {
                listing.root
            } else {
                format!(
                    "{}/{}:{}",
                    listing.root,
                    selected.get_untracked(),
                    line.get_untracked()
                )
            };
            wasm_bindgen_futures::spawn_local(async move {
                let result = wasm_bindgen_futures::JsFuture::from(
                    web_sys::window()
                        .unwrap()
                        .navigator()
                        .clipboard()
                        .write_text(&reference),
                )
                .await;
                match result {
                    Ok(_) => cx.notify("Copied reference".into(), NoticeKind::Success),
                    Err(_) => cx.notify(
                        "Could not copy; select and copy the displayed path instead.".into(),
                        NoticeKind::Error,
                    ),
                }
            });
        }
    };
    let files = Memo::new(move |_| {
        let query = filter.get().to_lowercase();
        ui.listing
            .get()
            .map(|l| {
                l.files
                    .into_iter()
                    .filter(|p| p.to_lowercase().contains(&query))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    view! {
        <section class="configuration" hidden=move || !active.get()>
            <div class="config-heading"><h1>"AI configuration"</h1><div class="actions">
                <Show when=move || desktop.get()><button on:click=open_folder disabled=move || !cx.connected.get() || !ui.listing.get().is_some_and(|l| l.exists)>"Open folder in Zed"</button></Show>
                <button on:click=refresh disabled=move || !cx.connected.get()>"Refresh"</button>
            </div></div>
            <p class="path">{move || ui.listing.get().map(|l| l.root).unwrap_or_default()}</p>
            <p class="config-note">"Personal rules apply when a task first captures its instructions; existing snapshots stay unchanged. Skills and roles here are not automatically loaded by Aide."</p>
            <p class="config-note">{move || if desktop.get() { "Edit or create files in Zed, then refresh." } else { "Edit files on this machine using SSH or your preferred remote editor, then refresh." }}</p>
            <p class="error" role="alert">{move || ui.list_error.get()}</p>
            <Show when=move || ui.listing.get().is_some_and(|l| !l.exists)><p>"The configuration folder does not exist yet. Create it at the path above in your external editor."</p></Show>
            <div class="config-layout">
                <aside class="config-sidebar"><label>"Find files"<input type="search" placeholder="Filter by filename or path" on:input=change_filter prop:value=move || filter.get()/></label>
                    <nav class="config-files" aria-label="Configuration files">
                        <For each=move || files.get() key=|path| path.clone() children=move |path| {
                            let id = path.clone();
                            let href = Section::link(&path);
                            view! { <a href=href aria-current=move || if selected.get() == id { Some("page") } else { None }>{path}</a> }
                        }/>
                    </nav>
                    <Show when=move || files.get().is_empty() && ui.listing.get().is_some()><p class="empty-line">"No matching files."</p></Show>
                </aside>
                <div class="config-viewer">
                    <div class="config-toolbar"><code>{move || selected.get()}</code><div class="actions">
                        <Show when=move || desktop.get()><button on:click=open_file disabled=move || selected.get().is_empty() || !cx.connected.get()>"Open in Zed"</button></Show>
                        <button on:click=copy_reference disabled=move || ui.listing.get().is_none()>{move || if selected.get().is_empty() { "Copy folder path" } else { "Copy reference" }}</button>
                    </div></div>
                    {move || match ui.document.get() {
                        DocumentState::Empty => view! { <p class="empty-line">"Select a file to view it."</p> }.into_any(),
                        DocumentState::Loading => view! { <p role="status">"Loading…"</p> }.into_any(),
                        DocumentState::Error(error) => view! { <p class="error" role="alert">{error}</p> }.into_any(),
                        DocumentState::Ready(doc) => view! { <div class="config-source" aria-label="File contents">{doc.text.split('\n').enumerate().map(|(index, text)| {
                            let number = index as u32 + 1;
                            let choose_line = move |_| line.set(number);
                            view! { <div class="config-line" class:selected=move || line.get() == number><button aria-label=format!("Select line {number}") aria-pressed=move || (line.get() == number).to_string() on:click=choose_line>{number}</button><code>{text.to_owned()}</code></div> }
                        }).collect_view()}</div> }.into_any(),
                    }}
                </div>
            </div>
        </section>
    }
}
