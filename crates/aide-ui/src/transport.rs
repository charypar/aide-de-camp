use crate::github::TaskUi;
use aide_core::{Command, Event, ProjectView};
use leptos::prelude::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::WebSocket;

#[derive(Clone, Copy)]
pub enum NoticeKind {
    Progress,
    Success,
    Error,
}

impl NoticeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Context {
    pub projects: RwSignal<Vec<ProjectView>>,
    pub connected: RwSignal<bool>,
    pub message: RwSignal<String>,
    pub notice_kind: RwSignal<NoticeKind>,
    notice_timeout: StoredValue<Option<gloo_timers::callback::Timeout>, LocalStorage>,
    pub task_ui: RwSignal<BTreeMap<String, TaskUi>>,
    owner: StoredValue<Owner>,
    sequence: RwSignal<u64>,
    socket: StoredValue<Rc<RefCell<Option<WebSocket>>>, LocalStorage>,
}
impl Context {
    pub fn new() -> Self {
        Self {
            projects: RwSignal::new(vec![]),
            connected: RwSignal::new(false),
            message: RwSignal::new(String::new()),
            notice_kind: RwSignal::new(NoticeKind::Progress),
            notice_timeout: StoredValue::new_local(None),
            task_ui: RwSignal::new(BTreeMap::new()),
            owner: StoredValue::new(Owner::current().expect("App owner")),
            sequence: RwSignal::new(0),
            socket: StoredValue::new_local(Rc::new(RefCell::new(None))),
        }
    }
    fn notify(self, message: String, kind: NoticeKind) {
        self.notice_kind.set(kind);
        self.message.set(message);
        self.notice_timeout.update_value(|timer| {
            // Dropping the previous timeout gives even identical messages a full lifetime.
            *timer = Some(gloo_timers::callback::Timeout::new(5000, move || {
                self.message.set(String::new());
            }));
        });
    }
    pub fn ui(self, root: &str, id: &str) -> TaskUi {
        let key = format!("{root}\0{id}");
        if let Some(ui) = self.task_ui.get_untracked().get(&key) {
            return *ui;
        }
        let ui = self.owner.with_value(|owner| owner.with(TaskUi::new));
        self.task_ui.update(|states| {
            states.insert(key, ui);
        });
        ui
    }
    pub fn next_request(self) -> String {
        self.sequence.update(|n| *n += 1);
        self.sequence.get_untracked().to_string()
    }
    pub fn send(self, command: Command) -> bool {
        let pr_command = matches!(
            command,
            Command::LinkPr { .. } | Command::UnlinkPr { .. } | Command::RefreshPrs { .. }
        );
        self.socket.with_value(|socket| {
            let result = socket
                .borrow()
                .as_ref()
                .filter(|s| s.ready_state() == WebSocket::OPEN)
                .map(|s| s.send_with_str(&serde_json::to_string(&command).unwrap()));
            if !matches!(result, Some(Ok(()))) {
                self.notify(
                    "Dashboard disconnected. Reconnect before trying again.".into(),
                    NoticeKind::Error,
                );
                false
            } else {
                if !pr_command {
                    self.notify("Working…".into(), NoticeKind::Progress);
                }
                true
            }
        })
    }
    pub fn connect(self) {
        let host = web_sys::window().unwrap().location().host().unwrap();
        let ws = WebSocket::new(&format!("ws://{host}/ws")).unwrap();
        self.socket
            .with_value(|s| *s.borrow_mut() = Some(ws.clone()));
        let open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| self.connected.set(true));
        ws.set_onopen(Some(open.as_ref().unchecked_ref()));
        open.forget();
        let message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
            move |event: web_sys::MessageEvent| {
                if let Some(text) = event.data().as_string()
                    && let Ok(event) = serde_json::from_str::<Event>(&text)
                {
                    match event {
                        Event::Snapshot { projects } => self.projects.set(projects),
                        Event::PrResult {
                            request_id,
                            message,
                            error,
                        } => {
                            for ui in self.task_ui.get_untracked().values() {
                                if ui.request.get_untracked() == request_id {
                                    ui.request.set(String::new());
                                    ui.error.set(error);
                                    ui.message.set(message.clone());
                                    if !error && ui.adding.get_untracked() {
                                        ui.draft.set(String::new());
                                        ui.linking.set(false);
                                    }
                                }
                            }
                        }
                        Event::Result { message, error } => {
                            let kind = if error {
                                NoticeKind::Error
                            } else {
                                NoticeKind::Success
                            };
                            self.notify(message, kind);
                        }
                    }
                }
            },
        );
        ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
        message.forget();
        let close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
            self.connected.set(false);
            for ui in self.task_ui.get_untracked().values() {
                if !ui.request.get_untracked().is_empty() {
                    ui.request.set(String::new());
                    ui.error.set(true);
                    ui.message.set(
                        "Disconnected; check the linked PRs after reconnecting before retrying."
                            .into(),
                    );
                }
            }
            gloo_timers::callback::Timeout::new(2000, move || self.connect()).forget();
        });
        ws.set_onclose(Some(close.as_ref().unchecked_ref()));
        close.forget();
    }
}
