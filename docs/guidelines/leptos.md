# Leptos guidelines

Scope: the browser playground in `crates/ardana-playground`, a Leptos 0.8 client-side-rendered (CSR) app compiled to
`wasm32-unknown-unknown`, bundled by trunk into `crates/ardana-playground/dist`, and embedded into the `ardana`
binary by `crates/ardana-server` (see `axum.md`). It covers components and signals, calls to the same-origin `/v1/*`
API, the in-tab engine that runs a library model's browser variant on onnxruntime-web (W2) with the engine module
`crates/ardana-engine` (the page's second wasm module, built by the same trunk run), the trunk and wasm
toolchain pinned through `cargo xtask fetch`, plain CSS, Jev share links, and the /impeccable design workflow that
gates every UI change (Q23, Q27).

## Versions
- `leptos` 0.8 — UI framework, feature `csr` only (docs.rs shows 0.8.21)
- `trunk` 0.21.14 — wasm bundler, installed into `tmp/bin` by `cargo xtask fetch`, run offline by `cargo xtask build`
- `wasm-bindgen` matches Cargo.lock — the crate version is resolved by leptos; the CLI in `tmp/bin` and `Trunk.toml` `[tools] wasm_bindgen` must equal the Cargo.lock version exactly
- `binaryen` version_133 — `wasm-opt` for release trunk builds; trunk 0.21.9's default `version_116` rejects rustc 1.97 output
- `lz-str` 0.2 — Rust port of lz-string's `compressToEncodedURIComponent` codec for Jev share links (docs.rs shows 0.2.1)
- `web-sys`, `js-sys` 0.3 and `wasm-bindgen-futures` 0.4 — the versions leptos already resolves (no new crates);
  the playground names them for `fetch`, `performance.now()`, `URLSearchParams` and the clipboard, with only the
  `web-sys` features it calls (`Clipboard`, `Document`, `Element`, `Headers`, `HtmlElement`, `HtmlInputElement`,
  `KeyboardEvent`, `Location`, `MediaQueryList`, `Navigator`, `Performance`, `Request`, `RequestInit`, `Response`,
  `ScrollBehavior`, `ScrollIntoViewOptions`, `ScrollLogicalPosition`, `UrlSearchParams`, `Window`, plus `DomRect`
  for the sidebar's section tracking, `Node` for the popover's outside-click check, `FocusEvent` for the focus
  leaving it, `DomTokenList` for the root's `tips-off`, `topbar-loose`, `banner-row` and `banner-loose` classes,
  `FocusOptions` for focus without scrolling and `Storage` for the remembered sidebar
  state; `ReadableStream`, `ReadableStreamDefaultReader` and `ReadableStreamReadResult` for a browser file's bytes as
  they arrive, `AbortController` and `AbortSignal` for a run in the tab that is stopped, `Cache` and `CacheStorage` for
  the files a tab keeps, `console` for the warning when it cannot, and `ResizeObserver` and `CssStyleDeclaration` for
  the top bar's and the banner's rendered heights)
- `ardana-engine` (workspace, `crates/ardana-engine`) — the engine module: `ardana-core`'s `Decider` as a wasm module
  of its own, its tokenizer on the pure-Rust `fancy-regex` backend on wasm32 (`huggingface.md`, "Tokenizers"); kept
  out of the page's wasm, which every visitor loads before the first paint, and imported on an "In browser" pick
- onnxruntime-web 1.30.0 — vendored in `crates/ardana-playground/ort/1.30.0/` (`onnx.md`, "onnxruntime-web"), not an
  npm dependency
- `serde_json` with `float_roundtrip` (the workspace feature, `rust.md`) — numbers read from a response keep their
  exact `f64`, so a re-serialised `data-value` equals the API's JSON text
- Onest Variable from `@fontsource-variable/onest` 5.3.1 and Geist Mono Variable from `@fontsource-variable/geist-mono`
  5.3.0 — ardana.ai's two faces (SIL OFL 1.1), vendored as woff2 files in `crates/ardana-playground/fonts/`, not
  npm dependencies; the versions are the ones the landing page's `package-lock.json` resolves

## Rules
- Depend on `leptos` with `features = ["csr"]` and nothing else from its mode set; exactly one of `csr`, `hydrate`,
  `ssr` may be on. The playground's one workspace dependency is `ardana-api`: the page's own wasm carries no core
  (`cargo xtask check-deps` enforces it). The in-tab engine's reader is `crates/ardana-engine`, a `cdylib` on
  `ardana-api` and `ardana-core` (Q9) that exports `profile`, `reader` and their objects (`Profile`, `Reader`, `Run`)
  with `#[wasm_bindgen]`; never pull the registry, the server, tokio, or anything that needs a filesystem into either
  (the core's file reads, the chat template's, are never called there). Its `Tokenizer` comes from
  `ardana_core::Tokenizer::from_bytes`, so it names no `tokenizers` features of its own.
- Enter through `fn main() { leptos::mount::mount_to_body(App) }` and `use leptos::prelude::*;` in each module.
- Components are `#[component] fn PascalName(..) -> impl IntoView`; props are function arguments. Keep one component
  per concern (state editor, questions editor/builder, model picker, results, raw panel, snippets, share).
- Pass signals or `move ||` closures into `view!` for anything that changes; `{sig.get()}` inside `view!` renders once
  and is a bug for live values. Attach handlers with `on:click=move |_| ..`.
- Create state with `signal(..)` / `RwSignal::new(..)`; derive values with closures or `Memo::new` when costly.
  Never use an `Effect` to copy one signal into another; effects are only for syncing with the outside world
  (URL hash, `localStorage`, DOM focus, a timer: the snippets wait for the editors to pause,
  `ui::snippets::shown_request`).
- Read without cloning where possible: `.read()` / `.with(..)` for large values (questions map, responses),
  `.get()` for small `Copy` values.
- The builder and the raw questions JSON share one source of truth: a parsed `ardana_api` questions map plus the
  raw text. Builder edits reserialize the text; valid text edits replace the map; invalid JSON sets an inline error
  and leaves the map untouched (R7.1). Builder edits go through `Deck::edit`/`Deck::edit_question` with the pure
  operations of `builder.rs`, which change only the keys they edit (a list of choice names stays a list until an
  option gets a description; a `{"0": ..}` score legend becomes a list once a level is edited; JSON structure in a
  field is shown read only). The text is `serde_json::to_string_pretty`, Jev's `JSON.stringify(q, null, 2)`.
- Type switches go through `Deck::switch_kind`, which keeps each question's criteria per type (`builder::KindMemory`)
  so a round trip through another type restores them. Preset loads and Remove question go through
  `Deck::load_preset` / `Deck::remove_question`, which keep both editors in `Deck::previous` for the console's
  Restore previous key until the next edit; `Deck::load_share` keeps them there too when they hold other work than
  the link's, with the picker's row, which Restore previous picks again (`Deck::pick`).
- Stale results: `Deck::sent` parses the exact body the last run sent; `inputs_changed` (state or model) and `stale`
  (anything) compare it with what Run would send now; a question is stale when its answer came from a different
  state, model or spec. Figures stay exact; only the tags change and the ink of the winning row goes gray.
- Focus is never dropped: Run (held while the questions JSON is invalid, the picked model runs only with the ardana
  CLI, or the questions name none, with the reason in the `#run-note` banner from `Deck::held`), Add option, Add level
  and every Remove button are held with `aria-disabled` and a guard (`Deck::run`, `ui::builder::remove_button`,
  `add_button`), never `disabled`; after a rename (the id field, or Instructions when the rename was committed by Tab),
  a removal, Add question, Add option or Add level (the new row's first field), Restore previous, a drawer pick or
  Stop leaving the banner while it had the focus (Run), `ui::focus_later(id)` focuses the next control on the next
  animation frame, once the re-keyed view exists. Element ids of a question come from `ui::dom_id`. Where the columns
  stack, a run's answers land a screen below Run, so `ui::show_result` scrolls the fault or the first question to the
  top and focuses its title (`.fault-title`, `.question-id`, both `tabindex="-1"`), as `ui::reveal` does for a section.
  The skip link handles its own click (`preventDefault`, then `reveal("content")`): its fragment would push a history
  entry whose Back loads the share link again. Under the open drawer the page and the skip link are `inert`; the Share
  popover closes on a `focusout` beyond `#share` and on Escape at the window. A relationship names an id only while
  its target exists (`aria-controls` of Edit and Share while open, Run's `aria-describedby` while the banner has its
  sentence).
- Announcements: two polite `role="status"` regions rendered empty from load (`run-status`: the run's outcome;
  `notice`: an input turning invalid or valid again, a preset loaded). Effects write them through `ui::announce`, not
  the view: words the region holds already are said again (another run's same outcome, the same preset loaded twice)
  by emptying it for a frame first. Field errors are text tied with `aria-describedby`, never `role="alert"`, so they
  do not repeat on every keystroke.
- A tooltip is drawn, never read out: `.tip::after { content: attr(data-tip) / "" }`, so it never joins a control's
  accessible name. A fact row is one Tab stop: its first name has `tabindex="0"`, the others `-1`, and
  `ui::questions::step_facts` moves the focus along the row on Left, Right, Home and End; the first name's `data-keys`
  says so in its tooltip under keyboard focus.
- Anything in `view!` that uses `<`, `>`, `<=` or `>=` in an attribute goes in braces:
  `disabled=move || { count.get() >= max }`. Unbraced, the macro ends the tag at `>` and turns the rest of the
  attribute into child nodes, running handler bodies at render.
- Lists whose rows hold inputs are keyed by something the input's own edits do not change (the channel by question
  id, renamed on `change` only; option and level rows by index), and each row's values are reactive closures over a
  `Memo` of its question, so typing never recreates the focused input.
- State text that parses as a JSON object or array is sent as JSON; every other text is sent as a string (R7.2).
- All network access goes through one `ApiClient` module with typed methods (`models()`, `systemone(req)`,
  `browser_profile(name, signal)`, `browser_file(name, file, from, version, signal)`, whose `api::Download` hands out
  the bytes as they arrive) using `ardana_api` request/response types; a browser file's failure says whether the
  connection or the server failed (`api::Failed`). `ApiClient::page()` builds it: served by `ardana serve`, it calls
  `/v1/*` paths on the page's own origin and nothing else, never a hard-coded host (Q15, R6.6). The standalone build
  (below) has no server: `ApiClient::standalone`, and `models()` GETs the library document (C1,
  `ardana_api::LibraryDocument`) from the URL the build named (`api::library`, the one `cfg(feature = "standalone")`
  of the crate: `ARDANA_PLAYGROUND_MODELS_URL` and `ARDANA_PLAYGROUND_HUB_URL`, read by `env!`) with
  `cache: no-cache` (a cached copy only once the host says it stands), parses it as this version reads it
  (`LibraryDocument::parse`, Q13 as `ardana` reads it: a family or a size it cannot read, a size not pinned to
  `hf.co/<org>/<repo>` at a 40-hex commit and a family whose name an earlier one took are left out, another `schema`
  refused), lists it as a server that has pulled none would (`api::listed`, Q18: one row per size under its canonical
  name `<family>:<size>` (`LibrarySize::name`) in the document's order, `x_pulled: false`, its default quant's
  bytes, the document's `default` and `browser_default` marked, every browser variant held) and keeps each size's
  `BrowserEntry`, by that name, for `browser_profile` (its `profile` and `commit`) and `browser_file`, which fetches
  `<hub>/<org>/<repo>/resolve/<commit>/<file>` with `Range` alone (a commit's file never changes; `If-Range` would
  make the request a CORS preflight) and no referrer (`ReferrerPolicy::NoReferrer`: huggingface.co's CDN answers a
  request from any page on `*.workers.dev` with a 404 that carries no CORS header). A document that does not arrive
  or read is `Err("The model library is unavailable: <reason>")`, and the page keeps its last list (`Deck::unlisted`;
  the sidebar's `#models-fault` alert says the reason, and while nothing is picked yet Run is held by
  `Held::Listing`, the select has no note and no `aria-describedby`, Snippets' lede is the "In browser" row's and its
  command reads `ardana run <model> --request -`). The page's own static files are loaded by their loaders, by paths relative to
  the page or to the loader, never absolute, so one `dist` runs wherever it is served: trunk's loader fetches the wasm,
  and `engine.js` imports the engine module from beside itself (`engine/ardana-engine.js`, whose `init` fetches its
  WASM beside it) and onnxruntime-web from `../ort/1.30.0/` (`import.meta.url`), whose bundles fetch their WASM
  modules beside them.
- Load `/v1/models` with a `LocalResource` (browser fetch futures are `!Send`); run decisions with
  `Action::new_local` over a `deck::Job` (the exact body, and for a run in the tab its model and `x_browser`),
  `dispatch` on the Run button, and drive the UI from `pending()` and `value()`. `Deck::run` is the one place a run
  happens and where it goes: the server (`ApiClient::systemone`) or, with an "In browser" row picked, the tab
  (`engine::run`); both yield an `api::Exchange` that records where it was answered (`Place::Server`,
  `Place::Tab(backend)`). Ctrl/Cmd+Enter
  presses Run from anywhere on the page and Ctrl/Cmd+\ toggles the sidebar, both through one `window_event_listener`
  on `keydown` in `ui::App`; a collapsed sidebar is remembered in `localStorage` (`ardana.sidebar`, web-sys
  `Storage`). The same listener scrolls a `<textarea>` that Tab lands on whole into view (`ui::show_text_field`, a
  frame later, `block: nearest`) when it fits below the bars: Chrome brings only a field's caret into view, which can
  leave its top under the sticky bars (WCAG 2.4.11); a field taller than that room keeps the browser's place.
- Keep the exact request JSON sent and the exact response body text received (status included) for the raw panel;
  show 422/413 `detail` as returned. Render an answer whose `type` is unknown as raw JSON instead of failing the
  whole response (R6.6).
- Time requests with `performance.now()` around the fetch, not `Date.now()`; R6.5 compares displayed latency with
  Playwright's timing within 20 ms.
- Every displayed API value carries the raw value in `data-value` (serialized from the response, unformatted) next
  to its formatted text: probabilities as percent with one decimal, confidence, noul and score with two decimals,
  token counts equal to `usage` (R6.5).
- Keep pure logic (share codec, state-mode detection, number formatting, builder edits, presets, snippet generation)
  in plain Rust modules with no DOM access, so it is unit-testable and reusable by the snippet and share code.
- The in-tab engine (`engine.rs`, `engine.js`, Q9): `GET /v1/browser/<name>/profile` (the server pulls the variant on
  the first request; the `ETag` is the version of its files), then the files from this browser's Cache Storage
  (`ardana-browser <name> <version>`, one cache per model and version; older versions are deleted once a run has every
  file of the current one) or downloaded with progress and kept. A download keeps what it has received as it goes, in
  parts of `engine::PART` (4 MiB) under `<file's URL>?bytes=<start>-<end>/<length>`, so one that stops (the connection
  drops, Stop, a reload) resumes in a later run: the parts kept from the first byte on are read back, and the rest is
  asked for with `Range: bytes=<had>-` and `If-Range: "<version>"`; a `206` is that rest, and a `200` (the server's copy
  changed) starts the file over and drops the parts. The bar counts the kept bytes as had. A file that arrives whole is
  kept whole and its parts deleted (a browser that refuses the whole file keeps its parts instead), so no file is kept
  twice, and only a whole file counts as kept (`holds`, `Files::keeps`). Cache Storage exists in a secure context only
  (`window.caches` reads `undefined` on an insecure page): there `engine::caches` gives none, the run downloads its
  files without keeping them, and nothing on the page says they are kept (`engine::keeps_files`). The engine module
  reads the profile before any file is fetched; the tokenizer comes first and the request is planned (`Reader::plan`,
  `Decider::plan` in the module) before the weights are fetched, so a refused request downloads only the tokenizer and
  answers with `DecideError::status` and `DecideError::body`, the server's own bytes. The page decodes each planned row
  (`Run::ids`, `Run::labels`, from `Decider::decodes`) on onnxruntime-web and hands its logits back (`Run::push`); the
  module replays them through `Decider::run` and serialises the response as axum's `Json` does (`Run::status`,
  `Run::body`). The model, its reader and its session stay loaded between runs, one model at a time, until a server row
  is picked (`Deck::pick` calls `engine::unload`: the session is released and the reader freed; a later run in the tab
  loads the model again from the kept files). Every object the engine module hands out holds that module's memory: the
  page frees it once done (`engine::Freed`, `free()` on drop). `engine.js` is a file of its own, which trunk copies
  beside the engine module (`rel="copy-file"` into `dist/engine/`, preloaded by a plain `modulepreload` link) and the
  page's wasm imports as a `raw_module` (`./engine/engine.js`, beside the page's glue), so the page has no wasm-bindgen
  snippet. It imports the
  engine module and onnxruntime-web on first use (or when an "In browser" row is picked, the page's opening pick among
  them) and holds the only JavaScript: the import of each module (a failed one is forgotten and asked for again under a
  new query), the session, one decode per row, and the release, which waits for a row still decoding (a run stopped
  mid-row leaves onnxruntime-web running it). A failed import of the engine module fails that run: "Ardana's engine did
  not load into this tab. Run again once the connection is back."
- A run in the tab is stopped by `Deck::stop` (the banner's Stop while the server pulls or the tab downloads, and
  picking another row at any stage): the action is aborted (`ActionAbortHandle`, so nothing it would have answered lands
  and Run is Run again), `engine::stop` aborts the run's `AbortController`, whose signal every browser-file request
  carries, and a session that finishes starting after the stop is released (`engine.js#createSession`). The run status
  then says "Stopped, not answered", and when Stop ended a download (`engine::stop` reads the run's stage) "Stopped, not
  answered. The next run resumes the download." (`engine::stopped`; where the page keeps no files: "… The next run
  starts the download over: a page without HTTPS keeps no files.").
- A run in the tab that gets no answer says why in the page's words (`api::Unanswered::advice`, one cause and one next
  step: run again once the connection is back, try again later, or reload the page when a retry cannot work, a build
  that loaded and could not start the model), under the title "Not answered in this tab."; the reason as the browser
  gave it is the raw exchange's. A download that stopped says where, and what the next run does with it: "The download
  of decider:0.8b stopped at 145.6 MB of 467.7 MB. Run again once the connection is back to resume it." (where the
  page keeps no files: "… to start it over: a page without HTTPS keeps no files.").
- Layout of `crates/ardana-playground/src`: `api.rs` (`ApiClient`, the only network code), `engine.rs` and `engine.js`
  (the in-tab engine; its reader is `crates/ardana-engine/src/lib.rs`), `share.rs` (decode and `link`), `request.rs`
  (state mode, questions parsing, the request and its body, editor texts, reading replies into typed answers or raw JSON
  and error `detail`s), `builder.rs` (question edits on raw specs), `presets.rs` (the three presets, request files in
  `presets/` plus `tests/fixtures/requests/ticket.json`), `snippets.rs` (curl, Python and TypeScript, and the ardana
  CLI's commands with ardana.ai's install line), `deck.rs` (`Deck`: every signal of the page, `Copy`, passed whole to
  components, plus the `Action` that runs and the `Memo` of the last run) and `ui/` with one module per region:
  `sidebar` (the logo, the model select, the presets, the page's sections), `topbar` (the sidebar opener and the logo
  while the sidebar is away, Share and its popover, Run, the banner under it), `state` (the state block), `questions`
  (the last run's line, the fault callout, one row per question with its type control, Edit and a bar per option, the
  Add question row, the questions JSON editor), `builder` (a question's builder fields), `exchange` and `snippets`
  (toggle blocks), `controls` (the segmented control and the copy key), `logo` (the brand lockup), `icons` and `figure`
  (the `data-value` figures). `ui::Shell` holds the sidebar's collapsed and drawer state.
- Figures: every API value on screen is a `ui::figure` element with `data-field` (a JSON pointer into the response),
  `data-value` (the raw value: a number as the shortest JSON text, a string as is) and `data-format` (`percent`,
  `fixed2`, `verbatim`). Formatting (`ardana_api::format`, shared with `ardana run`) rounds the number's decimal
  text half up (`round_decimal`), never the binary value, so `0.1235` shows `12.4%` on every platform; the Playwright
  helpers apply the same rule with BigInt.
- The page speaks only real model names; no text on it mentions Jev. The model picker lists `/v1/models`: the pulled
  models (`x_pulled` not `false`) in an optgroup "Pulled", then every model with an `x_browser` in "In browser · runs
  in this tab", labelled with that size, then the library models this server has not pulled in "Library · runs with
  the ardana CLI", labelled with their `x_size` (`decider:4b · 2.7 GB`, `ardana_api::human_size`); a share link naming
  a model the list lacks adds it as is. A model both pulled and browser-capable has a row in each group, and each row
  runs where it says (Q14). A server row's value is the model's name; an "In browser" row's is `<name> in-browser`
  (`deck::browser_value`: a word after a space, which no model name holds), and `Deck::pick` reads either into
  `model` and `in_browser`. The select's value follows the pick through an effect that sets it a frame after the
  options are drawn, not through `prop:value`: options are redrawn whenever the list changes, their elements reused by
  place, and a select keeps the selection its elements had (one removed with its group, one drawn over another's
  place), so a model pulled or removed meanwhile, listed again, left the select on its first row.
- `Deck::runs` says where Run answers the pick: `Tab` for an "In browser" row, else what the name is on the page
  (`deck::row`, Q20, the page's one rule for a name: the listed row whose name it equals, ASCII case ignored, and
  nothing else; the page never splits a name into a model and a tag, as the server and the CLI read it): `Server` for
  a row this server has pulled, and `Cli` for every other name, a row it has not pulled (one the server would pull) or
  one no row carries (a family, another quant, a former name: the CLI reads it, or refuses it, itself), and every
  server row of the standalone build (`Deck::standalone`, which no server serves). No pick on a server (no list to
  open on) is `Server`: the request names no model and the server answers with its default. Run never pulls (Q2): a
  `Cli` pick holds Run, autorun included, and the snippets show the ardana CLI's commands instead. `Deck::handoff`
  says which (`deck::Handoff`, Q4 as the user amended it): `Install` in the standalone build (ardana.ai's install line,
  then `ardana run`), `Pull` for any name but a row the server has pulled, an "In browser" row's model too
  (`ardana pull <pick as written>`, then `ardana run`; the server runs ardana already, so it never shows the install
  line), and `Run` for the "In browser" row of a model it has pulled (`ardana run` alone). Under the picker, a pick that
  does not run on the server says where it runs (`#model-note`, the select's description): "Runs in this tab. The first run downloads <size>, which this browser
  keeps. Running it takes 3 to 4 times that in memory; only a reload frees all of it." (where the page has no Cache
  Storage: "Runs in this tab. The first run on each visit downloads <size>: a page without HTTPS keeps no files. …"; the
  memory sentence is `ui::topbar::IN_MEMORY`, measured for every browser model on WebGPU and WASM, `onnx.md`), for a
  model the server has not pulled "Not pulled on this server. Pull it with the ardana CLI where the server runs
  (<x_size>), then Run answers here.", in the standalone build "Runs with the ardana CLI on your
  machine; its first run downloads <x_size>." (the size, `Deck::pull_size`, of the row the name is, none for a name no
  row carries).
- The picked model starts empty, and `Deck::place` places the page's first pick, and a share link's, once `/v1/models`
  is in (`Deck::unplaced`; a pick by hand ends the wait): no model (nothing linked, or Jev's `jev-*` alias) opens on
  `deck::opening_row` (the `x_default` model when it is pulled, else the `x_browser_default` model's "In browser" row,
  Q11, else the default model or the first pulled one), a linked model on `deck::linked_row` (through `deck::row`:
  the "In browser" row of the row it is when this server has not pulled that model and it has one, the only place it
  runs here; else that row, in the list's letters; else the name as it is). While
  nothing is picked a request names no model. The list is fetched again after every run and whenever the tab comes
  back (`focus` at the window, `visibilitychange` with the document shown: `ardana pull` or `ardana rm` may have
  changed the server's models since), so a model pulled meanwhile is a server row Run sends, without a reload. `?autorun=1` runs once after `/v1/models` answers (`Deck::autorun`): a
  server pick at once, a pick in the tab only when its files need no download (`engine::kept`); a first run in the tab
  waits for the tap.
- The banner `#run-note` under the top bar says, in this order: where a run in the tab is (below); why Run is held
  (`deck::Held`): in the fault colours "Questions JSON has an error"; quiet, "This server has not pulled <name>: pull it
  with the ardana CLI" (`Held::Pull`), or in the standalone build "<name> runs with the ardana CLI on
  your machine" (`Held::Cli`), followed by the ink "Show the command" key, whose arrow leads to the commands
  (`ui::reveal("snippets")`: the block opens, scrolls under the top bar and its summary takes focus; the sidebar's
  section rows use the same function), and after `Held::Cli` by "Run <browser default> in this tab instead"
  (`Deck::browser_default`), which picks that model's "In browser" row and focuses Run; quiet, in the standalone
  build while nothing is picked yet, "Loading the model library" or "The model library is unavailable"
  (`Held::Listing`, from `Deck::unlisted`); quiet, "Add a question to
  run"; else, quiet, what a run of the picked "In browser" row downloads while its files are not kept (`Deck::kept`,
  probed from Cache Storage whenever such a row is picked and after each run): "Run downloads <name> into this tab
  once: <size>, kept by this browser. Running it takes 3 to 4 times that in memory." (where the page has no Cache
  Storage: "Run downloads <name> into this tab on each visit: <size>, as a page without HTTPS keeps no files. …").
  Run's description is the banner's sentence alone (`#run-note-text`), never its bar or actions.
- Snippets (Q4): a `Server` pick shows curl, Python (`typesafe-sdk`) and TypeScript (`@typesafe-ai/sdk`) aimed at the
  page origin; a `Tab` or `Cli` pick shows the step `Deck::handoff` names, `ardana pull <name>` (`snippets::pull`)
  with what it downloads or ardana.ai's install line (`snippets::INSTALL`, its Windows line under it), and then
  `ardana run <name> --request -` with the exact body Run sends in a heredoc (`snippets::cli`), each in a command box
  with its copy key, and for `Install` what the CLI's first run downloads (`x_size`). The raw exchange's heading says
  where Run sends ("Sent · POST /v1/systemone", "Sent · in this tab", or "Sent" for a `Cli` pick, whose empty pane
  reads "Nothing sent yet."). The block builds its text only while it is open (its `toggle` event), and from editors
  holding more than 32 KB only once they have stayed unchanged for 150 ms (`leptos::prelude::debounce`), so a key
  typed in a long state never rebuilds and redraws a snippet; a copy key builds its text from the request as it stands
  when pressed.
- A run in the tab says where it is through `Deck::stage` (`engine::Stage`, set before the run is dispatched so the page
  never shows a server's wait for it), the Run label "Loading" unless said otherwise: nothing while it asks the server
  for the files it holds (`Asking`: it answers at once); "Pulling the browser files of <name> (<size>) on the server"
  with the label "Pulling" while the server pulls them, which the server says itself: the run passes the model's
  `x_browser_pulled` (`deck::Job`), and when the page's list says the server does not hold the files the run lists the
  models again before it asks for the profile (`engine::held`) and says Pulling only if they still are not held, never
  after a time; "Downloading <name> into this tab: <had> of <size>" with a `<progress>` bar (the answers' track and gray
  fill, redrawn every half percent, hidden from assistive technology) as one stage across the three files, from the
  first file a run lacks (the files it kept count as had); "Starting <name> in this tab" once it has every file, while
  it reads the kept ones, loads the tokenizer or starts onnxruntime-web; "Running <name> in this tab on WebGPU" (or
  WASM, "Running") while the rows decode. While the server pulls or the tab downloads, the banner ends in "Stop"; while
  a run is in flight it stays right under the top bar (`.banner.busy`, at the bar's measured height), and the scroll
  padding covers both. The polite `run-status` region says each stage once (`ui::topbar::tab_status`, a download in
  tenths: "Downloading <name> into this tab: 30% of <size>"), then the run's outcome. The answered line and the run
  status then say "Answered by <model> in this tab on <backend>", the raw exchange's heading "Sent · in this tab"
  (before a first run too, while an "In browser" row is picked), and the latency counts the plan, the decodes and the
  readout, not the download. Answers from a run in the tab are stale once the server row is picked, and the other way
  round. A held or busy Run shows no tooltip (`.run-key[aria-disabled]`, `[aria-busy]`): it would say "Send the state
  and questions" at the pill's faded strength, over the banner.
- Per-run visuals (the bars growing to their values) restart because each question's answer view is redrawn from
  the run number.
- Set dynamic CSS custom properties with a style tuple, `style=("--level", value.to_string())`; the rules that read
  them live in `.css`. The one exception is the root element, which no view renders: `ui::topbar::follow_heights` (a
  `ResizeObserver` on the top bar and the banner, and the window's `resize`) keeps `--topbar-height`,
  `--banner-height` and `--banner-row-height` (the banner's last row, its download bar and Stop on a row of their own)
  on it, and the classes `topbar-loose`, `banner-row` and `banner-loose` at what of them stays on screen
  (`ui::topbar::stuck`: at most a third of the viewport, so 400% zoom or enlarged text on a small screen keeps most of
  it). `base.css` turns them into `--topbar-stuck` and `--banner-stuck`, which `scroll-padding-top` (`base.css`, and
  `shell.css` while a run is in flight) and the sticky banner's `top` read, and the phone Share popover's `top` reads
  `--topbar-height`, so focus lands clear of what stays at every width, zoom and text size (WCAG 2.4.11) and the
  popover hangs under the bar however many rows it takes. Only the bar's and the sidebar head's `min-height` read the
  nominal `--topbar`.

## Share links
- Format: `#share/<compressToEncodedURIComponent(JSON)>`, payload
  `{"apiVersion":"v1","documentText","promptsText","selectedModels":["jev-latest"]}` exactly as Jev writes it.
- Encode with `lz_str::compress_to_encoded_uri_component(json_str)`. Decode with
  `lz_str::decompress_from_encoded_uri_component(s)`, which returns `Option<Vec<u16>>`; convert with
  `String::from_utf16` and treat `None`, invalid UTF-16 or bad JSON as a visible share-link error, never a panic.
- Read the hash on load and on `hashchange`; `location.hash` includes the leading `#`. `?autorun=1` runs the loaded
  share once after models load (R6.2 URLs). A link loaded over editors that hold other work keeps them, and the
  picker's row, for Restore previous (`Deck::load_share`); nothing on the page itself writes a fragment.
- `selectedModels` is optional when reading (a docs link has none); a link without one, or with a `jev-*` alias
  (`jev-latest`, `jev-1.12`), opens on the server's default model, so Jev's links keep working.
- The playground writes its own link with `share::link(origin, &SharePayload::new(state, questions, model))`, keys in
  Jev's order and the real model name in `selectedModels`; the share key opens it, recomputed from the editors while
  open, with a copy key.
- Round-trip test: the playground's own link must decode with lz-string 1.5.0 in Playwright (R7.4).

## Build: trunk, wasm-bindgen, wasm-opt
- `crates/ardana-playground/index.html` drives trunk. Every asset tag needs `data-trunk`:
  `<link data-trunk rel="rust" data-wasm-opt="z" />` for the crate and `<link data-trunk rel="css" href="styles/...css" />`
  per stylesheet. `data-wasm-opt` (`0`-`4`, `s`, `z`) only runs in `--release` builds.
- The engine module is a second `rel="rust"` link: `href="../ardana-engine" data-type="worker"
  data-bindgen-target="web" data-target-path="engine"`. A worker link injects no script and no preload (the page
  imports the module itself, on an "In browser" pick), is named after its crate without a content hash
  (`dist/engine/ardana-engine.js` and `ardana-engine_bg.wasm`), and `web` makes its glue an ES module whose default
  `init` fetches the WASM beside it. `engine.js` joins it there (`rel="copy-file" data-target-path="engine"`), with a
  plain `<link rel="modulepreload" href="engine/engine.js" />` so the page's glue does not wait a round trip for it.
  The link names no features (`data-cargo-features=""`): trunk passes `--features` to every Rust link, and the
  standalone build's feature is the page crate's alone.
  Names that never change need a fresh copy on every load: `ardana serve` sends `/engine/` with `Cache-Control:
  no-cache` (`axum.md`), so a page never runs with another binary's engine.
- No crate of the playground has a wasm-bindgen snippet (`#[wasm_bindgen(module = ...)]`, `inline_js`): wasm-bindgen
  names a snippet's directory by its crate's name and version alone, a name memory-serve would keep for a week across
  builds, and trunk copies wasm-bindgen's whole `snippets/` output into every Rust link's target path, the engine's
  too. Import JavaScript as `raw_module` from a file trunk copies into `dist/engine/`; `cargo xtask build` removes a
  `snippets/` an older build left in `target/wasm-bindgen/`.
- Unhashed static files go in with `rel="copy-dir"` (`fonts/`; `ort/`, onnxruntime-web) and `rel="copy-file"`
  (`brand/favicon.svg`, `brand/favicon.ico`), beside `index.html`; plain `<link>` tags without `data-trunk` name them
  by a path relative to the page (the favicons, the preload of the two latin font files), as `styles/fonts.css` names
  the fonts relative to itself, and the `embedded_binary` case fetches every such path. memory-serve gives these files its default week-long cache, so a
  changed font or icon takes a new file name, and a new onnxruntime-web a new version directory.
- `Trunk.toml` sits next to `index.html` and pins tools under `[tools]`: `wasm_bindgen = "<Cargo.lock version>"` and
  `wasm_opt = "version_133"`. Trunk uses a tool from `PATH` only when its version matches, otherwise it downloads.
  Bump the pin, `xtask/fetch.toml` (`cargo-lock:wasm-bindgen`) and `cargo xtask fetch` together whenever Cargo.lock's
  wasm-bindgen changes.
- The standalone build: `cargo xtask build-playground --public-url <path> [--hub <url>] [--library <url>]` runs
  trunk with `--features standalone --public-url <path> --dist tmp/playground/dist` (the embedded `dist/` stays as it
  is) and the two URLs the page reads at run time in its environment, `ARDANA_PLAYGROUND_HUB_URL` (default
  `https://huggingface.co`) and `ARDANA_PLAYGROUND_MODELS_URL` (default `/models.json`), which `api::library` reads
  with `env!` (cargo rebuilds the page when they change). No library is baked (Q13): the build reads no snapshot and
  no hub cache, and `xtask/src/playground.rs` "the standalone build bakes no library" holds it to that. trunk prefixes
  the paths it writes with `<path>`; every other path is relative. The page then lists the document it fetches, runs
  no model on a server: every server row runs with the ardana CLI after ardana.ai's install line, the browser default
  in the tab instead, and a share link keeps the page's path (`<origin><path>#share/…`). `../ardana-landing`'s
  `npm run build-playground` runs it and serves the result at `/playground/`, beside `/models.json`.
- Build locally only with `cargo xtask build`, which runs `trunk build --release --offline` through `Sandbox::command`
  (`PATH` starts with `tmp/bin`, `HOME` under `tmp/`). `--offline` turns a missing or mismatched tool into an error
  instead of a download into `~/Library/Caches/dev.trunkrs.trunk`, which the home guard would flag.
- Release builds run on GitHub runners, outside the `tmp/` sandbox (Q9): `.github/build-setup.yml`, which dist inserts
  into every build job of `.github/workflows/release.yml` before "Build artifacts", runs
  `rustup target add wasm32-unknown-unknown`, `cargo install trunk --version 0.21.14 --locked` and
  `trunk build --release` in `crates/ardana-playground`, online, so trunk downloads the `[tools]` pins (rustls build,
  assets for every runner platform). dist 0.32.0 drops a setup step's `working-directory`, so the step `cd`s itself.
- Never run trunk from a `build.rs` (outer cargo lock deadlock, rust-lang/cargo#8938) and never `cargo install trunk`
  into `~/.cargo/bin` on a developer machine; `trunk serve` is not part of the workflow, the playground is served by
  `ardana serve`.
- Keep `[build] filehash = true` (trunk's default) so hashed asset names let the server cache assets while
  `index.html` and `/engine/` stay uncached.
- Do not set `opt-level = "z"` on the workspace `[profile.release]`: it would also shrink-optimize the server binary.
  Size work for the wasm goes through `data-wasm-opt`, dependency choices, and brotli from memory-serve.

## CSS and design
- Styles live only in plain `.css` files under `crates/ardana-playground/styles/` (`fonts.css` for the `@font-face`
  rules; `base.css` for the light and dark tokens, reset, type, the logo and the caret; `controls.css` for buttons,
  the segmented control, fields, the select, tags, tooltips, the copy key and the toast; `shell.css` for the sidebar,
  the top bar, the banner, the share popover and the drawer; `page.css` for the page title, the columns, the state
  block, the question rows, bars, the fact row, callouts, toggle blocks and code blocks), linked with
  `<link data-trunk rel="css">`. No inline style strings, no CSS-in-Rust crates, no Tailwind: the /impeccable design
  hook scans `.css`, `.html`, `.ts`, `.js`, not `.rs`.
- The look is ardana.ai's (PRODUCT.md's brand commitment): `base.css` keeps the landing's token names and values
  (`--bg`, `--ink`, `--muted`, `--line`, `--accent`, `--code-bg`, ...) and adds only what the playground needs beyond
  them (`--line-strong`, `--track`, `--bar`, `--scrim`, `--danger*`). When the landing's `src/app.css` changes, change
  these with it.
- Type is the landing's: Onest Variable for words, Geist Mono Variable for anything a terminal or the API reads
  (code, model names, figures), self-hosted from `crates/ardana-playground/fonts/` (one woff2 per script subset with
  fontsource's `unicode-range`s in `fonts.css`, the OFL licences beside them), the latin files preloaded; nothing is
  loaded from another origin. Icons are inline SVG from `ui::icons::Icon` (one 16-unit grid, one round stroke,
  `currentColor`; the copy, check and arrow glyphs are the landing's), never Unicode glyphs or emoji.
- The logo and favicons are the Ardana logo kit's files, copied into `crates/ardana-playground/brand/` (provenance in
  its README) and regenerated from the kit, never drawn or edited by hand: `ui::logo::LogoSymbol` puts the lockup
  (`include_str!`) in the page once, its outer `svg` turned into a `<symbol>`, and each `ui::logo::Logo` draws it
  with `<use>` in `currentColor`, 26px tall (22px in a phone's top bar, 18px below 390px, above the kit's 80px minimum
  width). Size it through `.logo > svg` only: the lockup nests the mark's own `svg`, whose geometry must stay as drawn.
- The page's own words stay in the latin subsets of both faces: a shortcut is spelled out ("Ctrl+Enter or Cmd+Enter",
  never ⌘↵), so no other subset is fetched for the page's own text.
- Light and dark follow the system through `prefers-color-scheme` (`color-scheme: light dark`, the tokens redefined
  in one `@media` block in `base.css`); there is no theme control. The dark scheme is the logo kit's inverse: ink on
  `#0a0a0a`, the Run pill white. `forced-colors` blocks keep the checked segment, the bars, the primary button and the
  hairlines visible.
- Dynamic visuals (a bar's level) set a CSS custom property or a class from the view; the rules that use them stay in
  `.css`. Class names come from DESIGN.md's tokens and components. Motion moves `transform` and `opacity` only, never a
  layout property: a bar's fill is as long as its track and slides in to its level (`translateX`, clipped by the
  track), the drawer slides, and the wide sidebar's column collapses at once.
- Touch: under `(pointer: coarse)` every control reaches 44px by 44px (WCAG 2.5.5) without a change to its look: small
  keys, segments, toggle headings and banner actions through a transparent `::before` inset to
  `min(0px, (100% - 44px) / 2)`, rows, fields, the select, the share link's box and the banner by a 44px
  `min-height`. A fine pointer keeps the drawn sizes.
- Every UI change, from the first component on, goes through the /impeccable skill: PRODUCT.md, DESIGN.md and the
  `crates/ardana-playground` surface brief (Operate mode, six-block direction contract) exist before the first
  component; read the skill's `reference/craft-floor.md` before each UI edit; `buildPath` stays `"code"` with no image
  generation; the hook stays enabled (Q23, Q27).
- The Jev layout is a brand commitment: state on the left, questions and results on the right; the columns stack
  below 960px, where the sidebar becomes a drawer behind the top bar's opener.
- Because markup is rendered from `view!` in `.rs`, file scans see nothing; verify rendered UI with
  `impeccable detect --viewport 1280x800` and `--viewport 390x844` against the running server URLs (R6.2), plus
  Playwright screenshots under `tmp/screens/<case>/`.

## Testing
- Unit-test the pure modules with `cargo test -p ardana-playground` where they do not touch the DOM.
- Behavioural checks are Playwright suites in `e2e/playground` (`cargo xtask e2e playground`) against the embedded
  binary serving decider:2b from `tmp/hf`; never replace the model with canned responses in e2e. `browser_run` runs
  decider:0.8b's browser variant in the tab, on WebGPU and on WASM, `insecure_origin` on a page that is no secure
  context, `browser_stop` stops its download (and waits for a tap under `?autorun=1`), `browser_recover` runs again
  after a dropped connection, `first_run` opens an empty registry in the tab, `run_command` holds Run for every row
  the server has not pulled and every name no row carries, and `pull_while_open` runs the shown `ardana pull` while the page is
  open (`playwright.md`). The zoom and text-size reflow, the focus, announcement and ARIA cases, the touch reach and the
  first load's weight have cases of their own (`reflow.spec.ts`, `focus.spec.ts`, `polish.spec.ts`). The pure picker
  rules (`deck::row`, `deck::opening_row`, `deck::linked_row`, `deck::server_runs` for every kind of name, on a server and in the
  standalone build), the banner's and the status's words and what of the bars stays on screen (`ui::topbar`), the logo's
  symbol (`ui::logo`), the CLI's handoff and its note (`deck::handoff`, `ui::sidebar::cli_note`) and the ardana commands
  (`snippets::pull`, `snippets::cli`) have unit tests.

## Sources
- https://book.leptos.dev/getting_started/index.html — CSR setup: `csr` feature, trunk, wasm32 target, `mount_to_body`
- https://docs.rs/leptos/0.8 — feature flags (one of csr/hydrate/ssr), prelude, signals, resources, actions
- https://docs.rs/leptos/0.8/leptos/mount/fn.mount_to_body.html — `mount_to_body` signature
- https://docs.rs/leptos/0.8/leptos/prelude/struct.Action.html — `Action::new_local`, `dispatch`, `pending`, `value`
- https://book.leptos.dev/view/01_basic_component.html — `#[component]`, `view!`, reactive closures, `on:` handlers
- https://book.leptos.dev/reactivity/working_with_signals.html — `get`/`with`/`read`, derived signals, `Memo`
- https://book.leptos.dev/reactivity/14_create_effect.html — effects are for the outside world, not signal syncing
- https://book.leptos.dev/async/10_resources.html — `LocalResource` for `!Send` browser futures in CSR
- https://book.leptos.dev/async/13_actions.html — actions for user-triggered async calls
- https://book.leptos.dev/interlude_styling.html — plain CSS via `<link data-trunk rel="css">`
- https://book.leptos.dev/deployment/csr.html — `trunk build --release`, `dist`, SPA fallback to `index.html`
- https://book.leptos.dev/deployment/binary_size.html — wasm size levers, compression
- https://github.com/trunk-rs/trunk/blob/v0.21.14/guide/src/assets/index.md — `data-trunk`, `rel="rust"`, `data-wasm-opt` release-only, `rel="css"`, `rel="copy-file"` and `rel="copy-dir"` (copied exactly, no hashing)
- https://github.com/trunk-rs/trunk/blob/main/guide/src/configuration/index.md — config layering, `[tools]` PATH matching, offline errors
- https://github.com/trunk-rs/trunk/blob/v0.21.14/Trunk.toml — `[build] offline`, `filehash`, `minify`, `[tools]` keys
- https://github.com/trunk-rs/trunk/blob/main/guide/src/commands/index.md — `trunk build`, `trunk tools show`
- https://github.com/trunk-rs/trunk/releases/tag/v0.21.14 — 0.21.14 release notes
- https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/usage.html — CLI version must equal the crate version
- https://github.com/WebAssembly/binaryen/releases/tag/version_133 — binaryen `version_133` release
- https://docs.rs/lz-str/0.2 — `compress_to_encoded_uri_component`, `decompress_from_encoded_uri_component`, UTF-16 output
- https://developer.mozilla.org/en-US/docs/Web/API/Window/hashchange_event — `hashchange`, `location.hash` with `#`
- https://developer.mozilla.org/en-US/docs/Web/API/Performance/now — monotonic request timing
- https://developer.mozilla.org/en-US/docs/Web/API/Clipboard/writeText — the copy keys, a promise that may reject
- https://developer.mozilla.org/en-US/docs/Web/CSS/@font-face — the self-hosted faces, a variable `font-weight` range
- https://developer.mozilla.org/en-US/docs/Web/CSS/@font-face/unicode-range — one file per script subset, fetched on use
- https://developer.mozilla.org/en-US/docs/Web/CSS/@font-face/font-display — `swap`, as the landing loads them
- https://developer.mozilla.org/en-US/docs/Web/HTML/Attributes/rel/preload — `as="font"` preloads need `crossorigin`
- https://developer.mozilla.org/en-US/docs/Web/API/Cache — `match`, `put`: the files a tab keeps across visits
- https://developer.mozilla.org/en-US/docs/Web/API/Window/caches — `caches` exists in secure contexts only
- https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts — which pages are secure contexts
- https://developer.mozilla.org/en-US/docs/Web/API/AbortController — `abort()` and a request's `signal`: Stop
- https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Reference/Roles/status_role — polite status messages
- https://developer.mozilla.org/en-US/docs/Web/API/ReadableStreamDefaultReader/read — a download read chunk by chunk
- https://developer.mozilla.org/en-US/docs/Web/HTML/Element/progress — the download bar
- https://developer.mozilla.org/en-US/docs/Web/CSS/content — alternative text after `/`: a tooltip drawn, not read out
- https://developer.mozilla.org/en-US/docs/Web/CSS/@media/pointer — `pointer: coarse`: the 44px reach under a finger
- https://developer.mozilla.org/en-US/docs/Web/HTML/Global_attributes/inert — the page and the skip link under the drawer
- https://developer.mozilla.org/en-US/docs/Web/API/FocusEvent/relatedTarget — where the focus goes as it leaves the popover
- https://developer.mozilla.org/en-US/docs/Web/CSS/scroll-margin-bottom — room for a term's tooltip under it
- https://developer.mozilla.org/en-US/docs/Web/SVG/Element/symbol — the logo's one drawing, which each logo uses
- https://wasm-bindgen.github.io/wasm-bindgen/reference/js-snippets.html — snippets, named by their crate, which the playground avoids
- https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/on-js-imports/raw_module.html — `raw_module = "./engine/engine.js"`, an import written as is
- https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/on-js-imports/method.html — `method` bindings on the engine module's objects
- https://wasm-bindgen.github.io/wasm-bindgen/reference/deployment.html — `--target web`: an ES module whose `init` fetches the WASM
- https://developer.mozilla.org/en-US/docs/Web/HTML/Attributes/rel/modulepreload — `engine.js` fetched with the page's glue
