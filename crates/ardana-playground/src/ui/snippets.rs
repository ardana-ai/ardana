//! The snippets: a toggle block with the request Run would send now, as curl, Python or TypeScript aimed at this
//! server, in one command box with its copy key.

use leptos::prelude::*;

use super::controls::{CopyKey, Toggle};
use super::icons::Icon;
use crate::deck::Deck;
use crate::snippets::{Language, snippet};

#[component]
pub fn Snippets(deck: Deck) -> impl IntoView {
    let language = RwSignal::new(Language::Curl);
    let origin = window().location().origin().unwrap_or_default();
    let text = Memo::new(move |_| snippet(language.get(), &origin, &deck.request()));
    view! {
        <details class="toggle" id="snippets" open=true>
            <summary>
                <span class="toggle-marker">
                    <Icon name="chevron-right" />
                </span>
                <h2 class="toggle-heading">"Snippets"</h2>
                <span class="toggle-caption">"The request as it stands"</span>
            </summary>
            <div class="toggle-bar">
                <Toggle
                    legend="Language"
                    group="snippet-language".to_string()
                    options=Language::ALL.map(|l| (l, l.name())).to_vec()
                    checked=move |l| language.get() == l
                    pick=move |l| language.set(l)
                />
            </div>
            // The key comes first, as it did beside the language picker: Tab reaches it before the snippet.
            <div class="snippet-box">
                <CopyKey what="snippet" text=move || text.get() />
                <pre class="code-block wire snippet" tabindex="0" role="group" data-testid="snippet" data-language=move || language.get().name() aria-label=move || format!("{} snippet", language.get().name())>
                    {move || text.get()}
                </pre>
            </div>
        </details>
    }
}
