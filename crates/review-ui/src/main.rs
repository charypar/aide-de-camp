mod app;
mod chat;
mod context;
mod document_view;
mod dom;
mod feedback;
mod prompts;
mod reference;
mod selection;
mod transport;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
