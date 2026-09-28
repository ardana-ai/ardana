//! The Ardana playground: a Leptos CSR app built by `cargo xtask build` with trunk.

use leptos::prelude::*;

fn main() {
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"Ardana playground"</h1>
            <p>"The playground is not built yet."</p>
        </main>
    }
}
