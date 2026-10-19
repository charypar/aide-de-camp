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
    if let Some(el) = document().get_element_by_id("feedback-panel") {
        el.set_scroll_top(ui.feedback_scroll as i32);
    }
    if let Some(el) = document().get_element_by_id("reading") {
        el.set_scroll_top(ui.scroll as i32);
    }
    if let Some(el) = document().get_element_by_id("conversation") {
        el.set_scroll_top(ui.chat_scroll as i32);
    }
    highlight(cx);
}

pub(crate) fn highlight(cx: Context) {
    let ui = cx.ui.get_untracked();
    let reference = ui.selection;
    ProseHighlight::update(reference.as_ref(), ui.prose_offset);
    if let Ok(nodes) = document().query_selector_all(".code-line") {
        for i in 0..nodes.length() {
            let el = nodes.item(i).unwrap().dyn_into::<Element>().unwrap();
            let active = if let Some(Reference::Code {
                file,
                start,
                end,
                side,
            }) = &reference
            {
                let line = el
                    .get_attribute(&format!("data-{}-line", side.as_str()))
                    .filter(|number| !number.is_empty())
                    .or_else(|| {
                        (el.get_attribute("data-side").as_deref() == Some(side.as_str()))
                            .then(|| el.get_attribute("data-line"))
                            .flatten()
                    })
                    .and_then(|number| number.parse::<u32>().ok())
                    .unwrap_or(0);
                el.get_attribute("data-file").as_ref() == Some(file)
                    && line >= *start
                    && line <= *end
            } else {
                false
            };
            let _ = el.set_attribute("data-selected", if active { "true" } else { "false" });
        }
    }
}
/// Keep prose highlighted independently of the browser selection and input focus.
struct ProseHighlight;

impl ProseHighlight {
    fn update(reference: Option<&Reference>, offset: Option<u32>) {
        let _ = Self::paint(reference, offset);
    }

    fn paint(reference: Option<&Reference>, offset: Option<u32>) -> Option<()> {
        use js_sys::{Function, Reflect};
        use wasm_bindgen::JsValue;

        // A narrow binding avoids enabling all of web-sys's unstable APIs.
        let css = Reflect::get(&window(), &JsValue::from_str("CSS")).ok()?;
        let registry = Reflect::get(&css, &JsValue::from_str("highlights")).ok()?;
        let delete = Reflect::get(&registry, &JsValue::from_str("delete"))
            .ok()?
            .dyn_into::<Function>()
            .ok()?;
        let name = JsValue::from_str("review-reference");
        delete.call1(&registry, &name).ok()?;

        let Some(Reference::Prose { quote }) = reference else {
            return Some(());
        };
        let root = document().get_element_by_id("guide")?;
        let text = root.text_content()?;
        let units: Vec<u16> = text.encode_utf16().collect();
        let quote_units: Vec<u16> = quote.encode_utf16().collect();
        if quote_units.is_empty() {
            return None;
        }
        let start = offset
            .map(|n| n as usize)
            .filter(|start| {
                units.get(*start..*start + quote_units.len()) == Some(quote_units.as_slice())
            })
            .or_else(|| {
                units
                    .windows(quote_units.len())
                    .position(|part| part == quote_units)
            })?;
        let end = start + quote_units.len();

        let walker = document()
            .create_tree_walker_with_what_to_show(&root, 4)
            .ok()?;
        let range = document().create_range().ok()?;
        let mut position = 0;
        while let Ok(Some(node)) = walker.next_node() {
            let length = node
                .text_content()
                .unwrap_or_default()
                .encode_utf16()
                .count();
            if position <= start && start < position + length {
                range.set_start(&node, (start - position) as u32).ok()?;
            }
            if position < end && end <= position + length {
                range.set_end(&node, (end - position) as u32).ok()?;
                break;
            }
            position += length;
        }

        let constructor = Reflect::get(&window(), &JsValue::from_str("Highlight"))
            .ok()?
            .dyn_into::<Function>()
            .ok()?;
        let ranges = js_sys::Array::new();
        ranges.push(&range);
        let highlight = Reflect::construct(&constructor, &ranges).ok()?;
        let set = Reflect::get(&registry, &JsValue::from_str("set"))
            .ok()?
            .dyn_into::<Function>()
            .ok()?;
        set.call2(&registry, &name, &highlight).ok()?;
        Some(())
    }
}

#[derive(Clone, Debug)]
struct CodePoint {
    file: String,
    line: u32,
    side: DiffSide,
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
        })
    }

    fn reference(&self, other: &Self) -> Option<Reference> {
        (self.file == other.file && self.side == other.side).then(|| Reference::Code {
            file: self.file.clone(),
            start: self.line.min(other.line),
            end: self.line.max(other.line),
            side: self.side,
        })
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

    pub fn pointer_down(self, event: web_sys::MouseEvent) {
        if event.button() != 0 {
            return;
        }
        let preferred = if event.shift_key() {
            match self.cx.ui.get_untracked().code_anchor {
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
            match self.cx.ui.get_untracked().code_anchor {
                Some(Reference::Code {
                    file, start, side, ..
                }) if file == point.file && side == point.side => CodePoint {
                    file,
                    line: start,
                    side,
                },
                _ => point.clone(),
            }
        } else {
            point.clone()
        };
        self.cx.ui.update(|ui| {
            ui.code_anchor = anchor.reference(&anchor);
            ui.selection = anchor.reference(&point);
            ui.prose_offset = None;
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
            self.cx.ui.update(|ui| ui.selection = Some(reference));
        }
    }

    pub fn pointer_up(self, event: web_sys::MouseEvent) {
        if self.drag.get_value().is_some() {
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

        let Some(selection) = window().get_selection().ok().flatten() else {
            return;
        };
        let Some(range) = selection.get_range_at(0).ok() else {
            self.clear();
            return;
        };
        let Some(root) = document().get_element_by_id("guide") else {
            return;
        };
        // Range text preserves DOM offsets across paragraph boundaries and inline markup.
        // Browser Selection text can add layout newlines that aren't actual text nodes.
        let text = range.to_string().as_string().unwrap_or_default();
        let quote = text.trim().to_owned();
        if quote.is_empty() {
            self.clear();
            return;
        }
        if !root.contains(range.common_ancestor_container().ok().as_ref()) {
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
            ui.selection = Some(Reference::Prose { quote });
            ui.prose_offset = Some(offset as u32);
            ui.code_anchor = None;
        });
    }

    pub fn finish_drag(self) {
        if self.drag.get_value().is_some() {
            self.drag.set_value(None);
            self.cx.save();
        }
    }

    fn clear(self) {
        self.cx.change(|ui| {
            ui.selection = None;
            ui.prose_offset = None;
            ui.code_anchor = None;
        });
    }
}
