use crate::context::Context;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

pub(crate) fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}

pub(crate) fn document() -> web_sys::Document {
    window().document().unwrap()
}

pub(crate) fn id() -> String {
    format!("{}-{}", js_sys::Date::now(), js_sys::Math::random())
}

pub(crate) fn save_detail(cx: Context, key: &str, event: web_sys::Event) {
    if let Some(element) = event.target().and_then(|e| e.dyn_into::<Element>().ok()) {
        let open = element.has_attribute("open");
        if cx
            .ui
            .get_untracked()
            .details
            .get(key)
            .copied()
            .unwrap_or(false)
            != open
        {
            cx.change(|ui| {
                ui.details.insert(key.into(), open);
            });
        }
    }
}
