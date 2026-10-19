use crate::{context::Context, dom::window, selection::restore};
use leptos::prelude::*;
use review_core::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::WebSocket;

pub(crate) fn connect(cx: Context) {
    let host = window().location().host().unwrap();
    let ws = WebSocket::new(&format!("ws://{host}/ws")).unwrap();
    cx.socket.with_value(|s| *s.borrow_mut() = Some(ws.clone()));
    let first = Rc::new(RefCell::new(true));
    let onmessage =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            let Ok(event) = serde_json::from_str::<ServerEvent>(&text) else {
                return;
            };
            match event {
                ServerEvent::Snapshot { review } => {
                    let is_first = *first.borrow();
                    *first.borrow_mut() = false;
                    let changed = cx.review.get_untracked().is_none_or(|r| r.id != review.id);
                    if changed {
                        let pending = window()
                            .local_storage()
                            .ok()
                            .flatten()
                            .and_then(|s| {
                                s.get_item(&format!("review-outbox-{}", review.id))
                                    .ok()
                                    .flatten()
                            })
                            .and_then(|s| serde_json::from_str::<Vec<Envelope>>(&s).ok())
                            .unwrap_or_default();
                        cx.pending.with_value(|p| *p.borrow_mut() = pending);
                    }
                    let pending_ui = cx.pending.with_value(|p| {
                        p.borrow().iter().rev().find_map(|e| {
                            if let Command::SaveUi { ui } = &e.command {
                                Some(*ui.clone())
                            } else {
                                None
                            }
                        })
                    });
                    if is_first || changed {
                        cx.ui.set(pending_ui.unwrap_or_else(|| review.ui.clone()));
                    }
                    cx.review.set(Some(*review));
                    cx.connected.set(true);
                    if is_first {
                        cx.socket.with_value(|s| {
                            if let Some(ws) = s.borrow().as_ref() {
                                cx.pending.with_value(|p| {
                                    for c in p.borrow().iter() {
                                        let _ =
                                            ws.send_with_str(&serde_json::to_string(c).unwrap());
                                    }
                                });
                            }
                        });
                    }
                    if is_first || changed {
                        gloo_timers::callback::Timeout::new(150, move || restore(cx)).forget();
                    }
                }
                ServerEvent::Ack { id } => {
                    cx.pending
                        .with_value(|p| p.borrow_mut().retain(|e| e.id != id));
                    cx.store_pending();
                }
                ServerEvent::Error { id, message } => {
                    let failed = cx
                        .pending
                        .with_value(|p| p.borrow().iter().find(|e| e.id == id).cloned());
                    if let Some(envelope) = failed {
                        match envelope.command {
                            Command::Chat { text, .. } => cx.ui.update(|u| {
                                if u.chat_draft.is_empty() {
                                    u.chat_draft = text;
                                }
                            }),
                            Command::Feedback { text, id, .. } => cx.ui.update(|u| {
                                if u.feedback_draft.is_empty() {
                                    u.feedback_draft = text;
                                    u.editing_feedback = Some(id);
                                }
                            }),
                            _ => {}
                        }
                    }
                    cx.pending
                        .with_value(|p| p.borrow_mut().retain(|e| e.id != id));
                    cx.store_pending();
                    cx.error.set(message);
                }
            }
        });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
    let onclose = Closure::<dyn FnMut()>::new(move || {
        cx.connected.set(false);
        if cx
            .review
            .get_untracked()
            .is_none_or(|r| r.status != ReviewStatus::Completed)
        {
            gloo_timers::callback::Timeout::new(1200, move || connect(cx)).forget();
        }
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
}
