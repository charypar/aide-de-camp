use aide_core::{Command, Event, ProjectView};
use leptos::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::WebSocket;

#[derive(Clone, Copy)]
pub struct Context {
    pub projects: RwSignal<Vec<ProjectView>>,
    pub connected: RwSignal<bool>,
    pub message: RwSignal<String>,
    pub error: RwSignal<bool>,
    socket: StoredValue<Rc<RefCell<Option<WebSocket>>>, LocalStorage>,
}
impl Context {
    pub fn new() -> Self {
        Self {
            projects: RwSignal::new(vec![]),
            connected: RwSignal::new(false),
            message: RwSignal::new(String::new()),
            error: RwSignal::new(false),
            socket: StoredValue::new_local(Rc::new(RefCell::new(None))),
        }
    }
    pub fn send(self, command: Command) {
        self.socket.with_value(|socket| {
            let result = socket
                .borrow()
                .as_ref()
                .filter(|s| s.ready_state() == WebSocket::OPEN)
                .map(|s| s.send_with_str(&serde_json::to_string(&command).unwrap()));
            if !matches!(result, Some(Ok(()))) {
                self.error.set(true);
                self.message
                    .set("Dashboard disconnected. Reconnect before trying again.".into());
            } else {
                self.error.set(false);
                self.message.set("Working…".into());
            }
        });
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
                        Event::Result { message, error } => {
                            self.error.set(error);
                            self.message.set(message);
                        }
                    }
                }
            },
        );
        ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
        message.forget();
        let close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
            self.connected.set(false);
            gloo_timers::callback::Timeout::new(2000, move || self.connect()).forget();
        });
        ws.set_onclose(Some(close.as_ref().unchecked_ref()));
        close.forget();
    }
}
