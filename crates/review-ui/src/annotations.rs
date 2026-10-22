use crate::{
    context::Context,
    dom::{document, focus, window},
};
use leptos::prelude::*;
use review_core::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{DomRect, Element, Range};

pub(crate) fn prose_range(quote: &str, offset: Option<u32>) -> Option<Range> {
    let root = document().get_element_by_id("guide")?;
    let units: Vec<u16> = root.text_content()?.encode_utf16().collect();
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
            return Some(range);
        }
        position += length;
    }
    None
}

pub(crate) fn block_index(element: &Element) -> Option<u32> {
    let block = element.closest(".code-block").ok()??;
    let blocks = document().query_selector_all("#guide .code-block").ok()?;
    (0..blocks.length()).find(|i| {
        blocks
            .item(*i)
            .is_some_and(|el| el.is_same_node(Some(&block)))
    })
}

fn code_matches(el: &Element, reference: &Reference, block: Option<u32>) -> bool {
    if block.is_some() && block_index(el) != block {
        return false;
    }
    let Reference::Code {
        file,
        start,
        end,
        side,
    } = reference
    else {
        return false;
    };
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
    el.get_attribute("data-file").as_ref() == Some(file) && line >= *start && line <= *end
}

fn code_rows() -> Vec<Element> {
    document()
        .query_selector_all("#guide .code-line")
        .ok()
        .map(|nodes| {
            (0..nodes.length())
                .filter_map(|i| nodes.item(i)?.dyn_into::<Element>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn anchor(
    reference: Option<&Reference>,
    offset: Option<u32>,
    block: Option<u32>,
) -> Option<DomRect> {
    match reference? {
        Reference::Prose { quote } => Some(prose_range(quote, offset)?.get_bounding_client_rect()),
        reference @ Reference::Code { .. } => code_rows()
            .into_iter()
            .find(|el| code_matches(el, reference, block))
            .map(|el| el.get_bounding_client_rect()),
    }
}

fn paint(name: &str, ranges: Vec<Range>) -> Option<()> {
    use js_sys::{Function, Reflect};
    // CSS Custom Highlight is not yet exposed by stable web-sys.
    let css = Reflect::get(&window(), &JsValue::from_str("CSS")).ok()?;
    let registry = Reflect::get(&css, &JsValue::from_str("highlights")).ok()?;
    let constructor = Reflect::get(&window(), &JsValue::from_str("Highlight"))
        .ok()?
        .dyn_into::<Function>()
        .ok()?;
    let args = js_sys::Array::new();
    for range in ranges {
        args.push(&range);
    }
    let highlight = Reflect::construct(&constructor, &args).ok()?;
    let set = Reflect::get(&registry, &JsValue::from_str("set"))
        .ok()?
        .dyn_into::<Function>()
        .ok()?;
    set.call2(&registry, &JsValue::from_str(name), &highlight)
        .ok()?;
    Some(())
}

pub(crate) fn highlight(cx: Context) {
    let ui = cx.ui.get_untracked();
    let comments = cx.comments_untracked();
    for (category, name) in [
        (FeedbackCategory::Change, "review-changes"),
        (FeedbackCategory::Question, "review-questions"),
    ] {
        let ranges = comments
            .iter()
            .filter(|f| f.category == category)
            .filter_map(|f| match &f.reference {
                Some(Reference::Prose { quote }) => prose_range(quote, f.prose_offset),
                _ => None,
            })
            .collect();
        let _ = paint(name, ranges);
    }
    let active = match &ui.document.selection {
        Some(Reference::Prose { quote }) => prose_range(quote, ui.document.prose_offset)
            .into_iter()
            .collect(),
        _ => vec![],
    };
    let _ = paint("review-reference", active);
    for el in code_rows() {
        let active = ui
            .document
            .selection
            .as_ref()
            .is_some_and(|r| code_matches(&el, r, ui.document.code_block));
        let ids: Vec<_> = comments
            .iter()
            .filter(|f| {
                f.reference
                    .as_ref()
                    .is_some_and(|r| code_matches(&el, r, f.code_block))
            })
            .map(|f| f.id.as_str())
            .collect();
        let _ = el.set_attribute("data-selected", if active { "true" } else { "false" });
        let _ = el.set_attribute(
            "data-feedback",
            if ids.is_empty() { "false" } else { "true" },
        );
    }
    layout(cx);
}

pub(crate) fn layout(cx: Context) {
    let Some(sheet) = document().get_element_by_id("document-sheet") else {
        return;
    };
    let origin = sheet.get_bounding_client_rect().top();
    let mut cards: Vec<_> = cx
        .comments()
        .into_iter()
        .filter_map(|f| {
            let el = document().get_element_by_id(&format!("feedback-{}", f.id))?;
            let y = anchor(f.reference.as_ref(), f.prose_offset, f.code_block)
                .map(|r| r.top() - origin)
                .unwrap_or(0.);
            Some((y, el))
        })
        .collect();
    cards.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut bottom: f64 = 0.;
    for (y, el) in cards {
        let top = y.max(bottom);
        let _ = el.set_attribute("style", &format!("top:{top}px"));
        bottom = top + el.get_bounding_client_rect().height() + 12.;
    }
    let _ = sheet.set_attribute("style", &format!("min-height:{bottom}px"));
    position_toolbar(cx);
}

pub(crate) fn position_toolbar(cx: Context) {
    let Some(toolbar) = document().get_element_by_id("selection-actions") else {
        return;
    };
    let ui = cx.ui.get_untracked();
    let rect = anchor(
        ui.document.selection.as_ref(),
        ui.document.prose_offset,
        ui.document.code_block,
    );
    let reading = document()
        .get_element_by_id("reading")
        .map(|el| el.get_bounding_client_rect());
    if let (Some(rect), Some(reading)) = (rect, reading)
        && rect.bottom() >= reading.top()
        && rect.top() < reading.bottom()
    {
        let left = rect
            .left()
            .max(reading.left() + 8.)
            .min(reading.right() - 95.);
        let top = (rect.top() - 42.).max(reading.top() + 4.);
        let _ = toolbar.set_attribute("style", &format!("left:{left}px;top:{top}px"));
        return;
    }
    let _ = toolbar.set_attribute("style", "display:none");
}

pub(crate) fn focus_at(cx: Context, event: &web_sys::MouseEvent) -> bool {
    let x = event.client_x() as f64;
    let y = event.client_y() as f64;
    let row = event
        .target()
        .and_then(|t| t.dyn_into::<Element>().ok())
        .and_then(|el| el.closest(".code-line").ok().flatten());
    for comment in cx.comments_untracked() {
        let hit = match &comment.reference {
            Some(reference @ Reference::Code { .. }) => row
                .as_ref()
                .is_some_and(|row| code_matches(row, reference, comment.code_block)),
            Some(Reference::Prose { quote }) => prose_range(quote, comment.prose_offset)
                .and_then(|range| range.get_client_rects())
                .is_some_and(|rects| {
                    (0..rects.length())
                        .filter_map(|i| rects.item(i))
                        .any(|r| x >= r.left() && x <= r.right() && y >= r.top() && y <= r.bottom())
                }),
            _ => false,
        };
        if hit {
            cx.change(|ui| ui.feedback.focused = Some(comment.id.clone()));
            focus(&format!("feedback-{}", comment.id));
            return true;
        }
    }
    false
}
