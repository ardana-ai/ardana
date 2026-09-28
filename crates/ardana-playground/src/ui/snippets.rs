//! The snippets panel: the request RUN would send now, as curl, Python or TypeScript aimed at this server.

use leptos::prelude::*;

use super::controls::{CopyKey, Toggle};
use crate::deck::Deck;
use crate::snippets::{Language, snippet};

#[component]
pub fn Snippets(deck: Deck) -> impl IntoView {
    let language = RwSignal::new(Language::Curl);
    let origin = window().location().origin().unwrap_or_default();
    let text = Memo::new(move |_| snippet(language.get(), &origin, &deck.request()));
    view! {
        <section class="snippets" aria-labelledby="snippets-title">
            <div class="panel-head">
                <h2 id="snippets-title" class="engraved">"Snippets"</h2>
                <p class="legend">"The request as it stands"</p>
            </div>
            <div class="snippets-bar">
                <Toggle
                    legend="Language"
                    group="snippet-language".to_string()
                    options=Language::ALL.map(|l| (l, l.name())).to_vec()
                    checked=move |l| language.get() == l
                    pick=move |l| language.set(l)
                />
                <CopyKey what="snippet" text=move || text.get() />
            </div>
            <pre class="wire snippet" tabindex="0" data-testid="snippet" aria-label=move || format!("{} snippet", language.get().name())>
                {move || text.get()}
            </pre>
        </section>
    }
}
