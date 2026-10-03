//! The Ardana playground: a Leptos CSR app built by `cargo xtask build` with trunk and embedded into `ardana serve`.

mod api;
mod builder;
mod deck;
mod engine;
mod presets;
mod request;
mod share;
mod snippets;
mod ui;

fn main() {
    leptos::mount::mount_to_body(ui::App);
}
