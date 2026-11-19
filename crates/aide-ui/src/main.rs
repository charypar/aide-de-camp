mod app;
mod configuration;
mod github;
mod header;
mod transport;
fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
