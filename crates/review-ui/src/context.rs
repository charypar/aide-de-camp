use crate::dom::{focus, focus_without_scrolling, id, window};
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
            if r.status == ReviewStatus::Completed
                || self.pending.with_value(|p| p.borrow().is_empty())
            {
                let _ = storage.remove_item(&format!("review-outbox-{}", r.id));
                return;
            }
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
            || ui.chat.draft.trim().is_empty()
        {
            return;
        }
        self.send(Command::Chat {
            text: ui.chat.draft,
            reference: ui.chat.reference,
        });
        self.change(|ui| {
            ui.chat.draft.clear();
            ui.chat.paused = false;
        });
    }

    pub fn add_feedback(self) {
        let ui = self.ui.get_untracked();
        let Some(reference) = ui.document.selection else {
            return;
        };
        let id = id();
        self.change(|ui| {
            ui.feedback.drafts.insert(
                id.clone(),
                Feedback {
                    id: id.clone(),
                    text: String::new(),
                    reference: Some(reference),
                    category: FeedbackCategory::Change,
                    prose_offset: ui.document.prose_offset,
                    code_block: ui.document.code_block,
                },
            );
            ui.feedback.focused = Some(id.clone());
            ui.document.selection_actions = false;
            ui.document.selection = None;
            ui.document.code_anchor = None;
            ui.document.prose_offset = None;
            ui.document.code_block = None;
        });
        if let Ok(Some(selection)) = window().get_selection() {
            let _ = selection.remove_all_ranges();
        }
        focus_without_scrolling(&format!("feedback-input-{id}"));
    }

    pub fn discuss_selection(self) {
        self.change(|ui| {
            ui.chat.reference = ui.document.selection.clone();
            ui.chat.collapsed = false;
            ui.document.selection_actions = false;
        });
        focus("chat-input");
    }

    pub fn record_feedback(self) {
        let ui = self.ui.get_untracked();
        let Some(id) = ui.feedback.focused else {
            return;
        };
        let Some(feedback) = ui.feedback.drafts.get(&id).cloned() else {
            return;
        };
        if self.disabled() || feedback.text.trim().is_empty() {
            return;
        }
        self.send(Command::Feedback {
            id: id.clone(),
            text: feedback.text,
            reference: feedback.reference,
            category: feedback.category,
            prose_offset: feedback.prose_offset,
            code_block: feedback.code_block,
        });
        self.change(|ui| {
            ui.feedback.drafts.remove(&id);
        });
    }

    pub fn discard_draft(self, id: &str) {
        self.change(|ui| {
            ui.feedback.drafts.remove(id);
        });
    }

    pub fn delete_feedback(self, id: String) {
        self.change(|ui| {
            ui.feedback.drafts.remove(&id);
            if ui.feedback.focused.as_ref() == Some(&id) {
                ui.feedback.focused = None;
            }
        });
        self.send(Command::DeleteFeedback { id });
    }

    pub fn comments(self) -> Vec<Feedback> {
        self.review.track();
        self.ui.track();
        self.comments_untracked()
    }

    pub fn comments_untracked(self) -> Vec<Feedback> {
        let mut comments = self
            .review
            .get_untracked()
            .map(|r| r.feedback)
            .unwrap_or_default();
        for (id, draft) in self.ui.get_untracked().feedback.drafts {
            if let Some(existing) = comments.iter_mut().find(|f| f.id == id) {
                *existing = draft;
            } else {
                comments.push(draft);
            }
        }
        comments
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
        let id = feedback.id.clone();
        self.change(|ui| {
            ui.feedback.focused = Some(id.clone());
            ui.feedback.drafts.entry(id.clone()).or_insert(feedback);
        });
        focus_without_scrolling(&format!("feedback-input-{id}"));
    }

    pub fn clear_reference(self) {
        self.change(|ui| ui.chat.reference = None);
    }

    pub fn use_file_range(self) {
        let ui = self.ui.get_untracked();
        let start = ui.chat.manual_start.parse().unwrap_or(1);
        let reference = Reference::Code {
            file: ui.chat.manual_file,
            start,
            end: ui.chat.manual_end.parse().unwrap_or(start),
            side: ui.chat.manual_side,
        };
        if let Some(review) = self.review.get_untracked() {
            match validate_reference(&reference, &review.workspace, &review.guide) {
                Ok(()) => self.change(|ui| {
                    ui.chat.reference = Some(reference);
                }),
                Err(error) => self.error.set(error),
            }
        }
    }
}
