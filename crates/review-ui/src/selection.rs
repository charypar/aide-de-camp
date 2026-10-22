use crate::{
    context::Context,
    dom::{document, window},
};
use leptos::prelude::*;
use review_core::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

pub(crate) fn restore(cx: Context) {
    let ui = cx.ui.get_untracked();
    if let Some(el) = document().get_element_by_id("reading") {
        el.set_scroll_top(ui.document.scroll as i32);
    }
    if let Some(el) = document().get_element_by_id("conversation") {
        el.set_scroll_top(ui.chat.scroll as i32);
    }
    crate::annotations::highlight(cx);
}

#[derive(Clone, Debug)]
struct CodePoint {
    file: String,
    line: u32,
    side: DiffSide,
    block: u32,
}

impl CodePoint {
    fn from_target(event: &web_sys::MouseEvent, preferred: Option<DiffSide>) -> Option<Self> {
        let target = event.target()?.dyn_into::<Element>().ok()?;
        let row = target.closest(".code-line").ok()??;
        let side = preferred.or_else(|| row.get_attribute("data-side")?.parse().ok())?;
        let line = row
            .get_attribute(&format!("data-{}-line", side.as_str()))
            .filter(|number| !number.is_empty())
            .or_else(|| {
                (row.get_attribute("data-side").as_deref() == Some(side.as_str()))
                    .then(|| row.get_attribute("data-line"))
                    .flatten()
            })?;
        Some(Self {
            file: row.get_attribute("data-file")?,
            line: line.parse().ok()?,
            side,
            block: crate::annotations::block_index(&row)?,
        })
    }

    fn reference(&self, other: &Self) -> Option<Reference> {
        (self.file == other.file && self.side == other.side && self.block == other.block).then(
            || Reference::Code {
                file: self.file.clone(),
                start: self.line.min(other.line),
                end: self.line.max(other.line),
                side: self.side,
            },
        )
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SelectionController {
    cx: Context,
    drag: StoredValue<Option<CodePoint>>,
}

impl SelectionController {
    pub fn new(cx: Context) -> Self {
        Self {
            cx,
            drag: StoredValue::new(None),
        }
    }

    fn block_reference(event: &web_sys::MouseEvent) -> Option<Reference> {
        let target = event.target()?.dyn_into::<Element>().ok()?;
        let header = target.closest(".code-reference").ok()??;
        Some(Reference::Code {
            file: header.get_attribute("data-file")?,
            start: header.get_attribute("data-start")?.parse().ok()?,
            end: header.get_attribute("data-end")?.parse().ok()?,
            side: header.get_attribute("data-side")?.parse().ok()?,
        })
    }

    pub fn pointer_down(self, event: web_sys::MouseEvent) {
        if event.button() != 0 {
            return;
        }
        if let Some(reference) = Self::block_reference(&event) {
            event.prevent_default();
            if let Ok(Some(selection)) = window().get_selection() {
                let _ = selection.remove_all_ranges();
            }
            self.cx.change(|ui| {
                ui.document.selection = Some(reference);
                ui.document.selection_actions = true;
                ui.document.prose_offset = None;
                ui.document.code_anchor = None;
                ui.document.code_block = event
                    .target()
                    .and_then(|t| t.dyn_into::<Element>().ok())
                    .and_then(|el| crate::annotations::block_index(&el));
            });
            return;
        }
        let preferred = if event.shift_key() {
            match self.cx.ui.get_untracked().document.code_anchor {
                Some(Reference::Code { side, .. }) => Some(side),
                _ => None,
            }
        } else {
            None
        };
        let Some(point) = CodePoint::from_target(&event, preferred) else {
            return;
        };
        event.prevent_default();
        if let Ok(Some(selection)) = window().get_selection() {
            let _ = selection.remove_all_ranges();
        }

        let anchor = if event.shift_key() {
            match self.cx.ui.get_untracked().document.code_anchor {
                Some(Reference::Code {
                    file, start, side, ..
                }) if file == point.file
                    && side == point.side
                    && self.cx.ui.get_untracked().document.code_block == Some(point.block) =>
                {
                    CodePoint {
                        file,
                        line: start,
                        side,
                        block: point.block,
                    }
                }
                _ => point.clone(),
            }
        } else {
            point.clone()
        };
        self.cx.ui.update(|ui| {
            ui.document.code_anchor = anchor.reference(&anchor);
            ui.document.code_block = Some(point.block);
            ui.document.selection = anchor.reference(&point);
            ui.document.selection_actions = false;
            ui.document.prose_offset = None;
        });
        self.drag.set_value(Some(anchor));
    }

    pub fn pointer_move(self, event: web_sys::MouseEvent) {
        if event.buttons() & 1 == 0 {
            self.finish_drag();
            return;
        }
        let Some(anchor) = self.drag.get_value() else {
            return;
        };
        let Some(point) = CodePoint::from_target(&event, Some(anchor.side)) else {
            return;
        };
        if let Some(reference) = anchor.reference(&point) {
            self.cx
                .ui
                .update(|ui| ui.document.selection = Some(reference));
        }
    }

    pub fn pointer_up(self, event: web_sys::MouseEvent) {
        if self.drag.get_value().is_some() {
            if !event.shift_key()
                && self
                    .cx
                    .ui
                    .get_untracked()
                    .document
                    .selection
                    .as_ref()
                    .is_some_and(
                        |r| matches!(r, Reference::Code { start, end, .. } if start == end),
                    )
            {
                crate::annotations::focus_at(self.cx, &event);
            }
            self.finish_drag();
            return;
        }
        let Some(target) = event
            .target()
            .and_then(|node| node.dyn_into::<Element>().ok())
        else {
            return;
        };
        if let Ok(Some(button)) = target.closest("[data-copy]") {
            if let Some(text) = button.get_attribute("data-copy") {
                let _ = window().navigator().clipboard().write_text(&text);
            }
            return;
        }
        if target.closest(".code-reference").ok().flatten().is_some() {
            return;
        }

        let Some(selection) = window().get_selection().ok().flatten() else {
            return;
        };
        let Some(range) = selection.get_range_at(0).ok() else {
            crate::annotations::focus_at(self.cx, &event);
            self.clear();
            return;
        };
        let Some(root) = document().get_element_by_id("guide") else {
            return;
        };
        // Triple-clicking a paragraph at a prose/code boundary can extend the DOM
        // range into the next, unselectable code header. Keep only the visible text.
        if let Ok(start) = range.start_container()
            && let Some(prose) = start
                .parent_element()
                .and_then(|el| el.closest(".prose").ok().flatten())
            && !prose.contains(range.end_container().ok().as_ref())
        {
            let clipped = range.clone_range();
            let _ = clipped.set_end(&prose, prose.child_nodes().length());
            let normalize = |text: String| text.split_whitespace().collect::<Vec<_>>().join(" ");
            if normalize(clipped.to_string().as_string().unwrap_or_default())
                == normalize(selection.to_string().as_string().unwrap_or_default())
            {
                let _ = range.set_end(&prose, prose.child_nodes().length());
            }
        }
        // Range text preserves DOM offsets across paragraph boundaries and inline markup.
        // Browser Selection text can add layout newlines that aren't actual text nodes.
        let text = range.to_string().as_string().unwrap_or_default();
        let quote = text.trim().to_owned();
        if quote.is_empty() {
            crate::annotations::focus_at(self.cx, &event);
            self.clear();
            return;
        }
        if !root.contains(range.common_ancestor_container().ok().as_ref()) {
            return;
        }
        let reference = Reference::Prose {
            quote: quote.clone(),
        };
        if self.cx.review.get_untracked().is_none_or(|review| {
            validate_reference(&reference, &review.workspace, &review.guide).is_err()
        }) {
            crate::annotations::focus_at(self.cx, &event);
            self.clear();
            return;
        }

        let prefix = document().create_range().unwrap();
        let _ = prefix.select_node_contents(&root);
        let Ok(start_node) = range.start_container() else {
            return;
        };
        let _ = prefix.set_end(&start_node, range.start_offset().unwrap_or(0));
        let offset = prefix
            .to_string()
            .as_string()
            .unwrap_or_default()
            .encode_utf16()
            .count()
            + text[..text.len() - text.trim_start().len()]
                .encode_utf16()
                .count();
        self.cx.change(|ui| {
            ui.document.selection = Some(Reference::Prose { quote });
            ui.document.selection_actions = true;
            ui.document.prose_offset = Some(offset as u32);
            ui.document.code_block = None;
            ui.document.code_anchor = None;
        });
    }

    pub fn finish_drag(self) {
        if self.drag.get_value().is_some() {
            self.drag.set_value(None);
            self.cx.change(|ui| ui.document.selection_actions = true);
        }
    }

    fn clear(self) {
        self.cx.change(|ui| {
            ui.document.selection = None;
            ui.document.code_block = None;
            ui.document.selection_actions = false;
            ui.document.prose_offset = None;
            ui.document.code_anchor = None;
        });
    }
}
