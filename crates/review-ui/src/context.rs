use crate::dom::{id, window};
use leptos::prelude::*;
use review_core::*;
use std::{cell::RefCell, rc::Rc};
use web_sys::WebSocket;

#[derive(Clone, Copy)]
pub(crate) struct Context {
    pub(crate) review: RwSignal<Option<Review>>,
    pub(crate) ui: RwSignal<UiState>,
    pub(crate) connected: RwSignal<bool>,
    pub(crate) error: RwSignal<String>,
    pub(crate) socket: StoredValue<Rc<RefCell<Option<WebSocket>>>, LocalStorage>,
    pub(crate) pending: StoredValue<Rc<RefCell<Vec<Envelope>>>, LocalStorage>,
}

impl Context {
    pub(crate) fn send(self, command: Command) {
        let Some(review) = self.review.get_untracked() else {
            return;
        };
        if review.status == ReviewStatus::Completed {
            return;
        }
        let envelope = Envelope {
            id: id(),
            review_id: review.id,
            command,
        };
        self.pending
            .with_value(|p| p.borrow_mut().push(envelope.clone()));
        self.store_pending();
        self.socket.with_value(|s| {
            if let Some(ws) = s.borrow().as_ref()
                && ws.ready_state() == WebSocket::OPEN
            {
                let _ = ws.send_with_str(&serde_json::to_string(&envelope).unwrap());
            }
        });
    }

    pub(crate) fn store_pending(self) {
        if let Some(r) = self.review.get_untracked()
            && let Ok(Some(storage)) = window().local_storage()
        {
            let _ = storage.set_item(
                &format!("review-outbox-{}", r.id),
                &self
                    .pending
                    .with_value(|p| serde_json::to_string(&*p.borrow()).unwrap()),
            );
        }
    }

    pub(crate) fn save(self) {
        self.send(Command::SaveUi {
            ui: Box::new(self.ui.get_untracked()),
        });
    }

    pub(crate) fn change(self, f: impl FnOnce(&mut UiState)) {
        self.ui.update(f);
        self.save();
    }
}

impl Context {
    pub fn disabled(self) -> bool {
        !self.connected.get()
            || self
                .review
                .get()
                .is_none_or(|review| review.status == ReviewStatus::Completed)
    }

    pub fn send_chat(self) {
        let ui = self.ui.get_untracked();
        if self.disabled()
            || self.review.get_untracked().is_none_or(|r| r.busy)
            || ui.chat_draft.trim().is_empty()
        {
            return;
        }
        self.send(Command::Chat {
            text: ui.chat_draft,
            reference: ui.selection,
        });
        self.ui.update(|ui| ui.chat_draft.clear());
    }

    pub fn record_feedback(self) {
        let ui = self.ui.get_untracked();
        if self.disabled() || ui.feedback_draft.trim().is_empty() {
            return;
        }
        self.send(Command::Feedback {
            id: ui.editing_feedback.unwrap_or_else(id),
            text: ui.feedback_draft,
            reference: ui.selection,
        });
        self.ui.update(|ui| {
            ui.feedback_draft.clear();
            ui.editing_feedback = None;
        });
    }

    pub fn composer_key(self, event: web_sys::KeyboardEvent, feedback: bool) {
        if event.key() != "Enter"
            || event.shift_key()
            || event.is_composing()
            || event.key_code() == 229
        {
            return;
        }
        event.prevent_default();
        if event.repeat() {
            return;
        }
        if feedback {
            self.record_feedback();
        } else {
            self.send_chat();
        }
    }

    pub fn edit_feedback(self, feedback: Feedback) {
        self.change(|ui| {
            ui.editing_feedback = Some(feedback.id);
            ui.feedback_draft = feedback.text;
            ui.prose_offset = None;
            ui.code_anchor = None;
            ui.selection = feedback.reference;
        });
    }

    pub fn clear_reference(self) {
        if let Ok(Some(selection)) = window().get_selection() {
            let _ = selection.remove_all_ranges();
        }
        self.change(|ui| {
            ui.selection = None;
            ui.prose_offset = None;
            ui.code_anchor = None;
        });
    }

    pub fn use_file_range(self) {
        let ui = self.ui.get_untracked();
        let start = ui.manual_start.parse().unwrap_or(1);
        let reference = Reference::Code {
            file: ui.manual_file,
            start,
            end: ui.manual_end.parse().unwrap_or(start),
            side: ui.manual_side,
        };
        if let Some(review) = self.review.get_untracked() {
            match validate_reference(&reference, &review.workspace, &review.guide) {
                Ok(()) => self.change(|ui| {
                    ui.code_anchor = Some(reference.clone());
                    ui.selection = Some(reference);
                }),
                Err(error) => self.error.set(error),
            }
        }
    }
}
