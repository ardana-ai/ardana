# Leptos guidelines

Scope: the browser playground in `crates/ardana-playground`, a Leptos 0.8 client-side-rendered (CSR) app compiled to
`wasm32-unknown-unknown`, bundled by trunk into `crates/ardana-playground/dist`, and embedded into the `ardana`
binary by `crates/ardana-server` (see `axum.md`). It covers components and signals, calls to the same-origin `/v1/*`
API, the trunk and wasm toolchain pinned through `cargo xtask fetch`, plain CSS, Jev share links, and the /impeccable
design workflow that gates every UI change (Q23, Q27).

## Versions
- `leptos` 0.8 — UI framework, feature `csr` only (docs.rs shows 0.8.21)
- `trunk` 0.21.14 — wasm bundler, installed into `tmp/bin` by `cargo xtask fetch`, run offline by `cargo xtask build`
- `wasm-bindgen` matches Cargo.lock — the crate version is resolved by leptos; the CLI in `tmp/bin` and `Trunk.toml` `[tools] wasm_bindgen` must equal the Cargo.lock version exactly
- `binaryen` version_133 — `wasm-opt` for release trunk builds; trunk 0.21.9's default `version_116` rejects rustc 1.97 output
- `lz-str` 0.2 — Rust port of lz-string's `compressToEncodedURIComponent` codec for Jev share links (docs.rs shows 0.2.1)

## Rules
- Depend on `leptos` with `features = ["csr"]` and nothing else from its mode set; exactly one of `csr`, `hydrate`,
  `ssr` may be on. The only workspace dependency is `ardana-api` (`cargo xtask check-deps` enforces it); never pull
  in `ardana-core`, tokio, or anything that needs a filesystem.
- Enter through `fn main() { leptos::mount::mount_to_body(App) }` and `use leptos::prelude::*;` in each module.
- Components are `#[component] fn PascalName(..) -> impl IntoView`; props are function arguments. Keep one component
  per concern (state editor, questions editor/builder, model picker, results, raw panel, snippets, share).
- Pass signals or `move ||` closures into `view!` for anything that changes; `{sig.get()}` inside `view!` renders once
  and is a bug for live values. Attach handlers with `on:click=move |_| ..`.
- Create state with `signal(..)` / `RwSignal::new(..)`; derive values with closures or `Memo::new` when costly.
  Never use an `Effect` to copy one signal into another; effects are only for syncing with the outside world
  (URL hash, `localStorage`, DOM focus).
- Read without cloning where possible: `.read()` / `.with(..)` for large values (questions map, responses),
  `.get()` for small `Copy` values.
- The builder and the raw questions JSON share one source of truth: a parsed `ardana_api` questions map plus the
  raw text. Builder edits reserialize the text; valid text edits replace the map; invalid JSON sets an inline error
  and leaves the map untouched (R7.1).
- State text that parses as a JSON object or array is sent as JSON; every other text is sent as a string (R7.2).
- All network access goes through one `ApiClient { base_url }` module with typed methods (`models()`, `systemone(req)`)
  using `ardana_api` request/response types. Construct it with the page origin; never hard-code a host, never call
  any non-`/v1/*` URL (Q15, R6.6).
- Load `/v1/models` with a `LocalResource` (browser fetch futures are `!Send`); run decisions with
  `Action::new_local`, `dispatch` on the Run button, and drive the UI from `pending()` and `value()`.
- Keep the exact request JSON sent and the exact response body text received (status included) for the raw panel;
  show 422/413 `detail` as returned. Render an answer whose `type` is unknown as raw JSON instead of failing the
  whole response (R6.6).
- Time requests with `performance.now()` around the fetch, not `Date.now()`; R6.5 compares displayed latency with
  Playwright's timing within 20 ms.
- Every displayed API value carries the raw value in `data-value` (serialized from the response, unformatted) next
  to its formatted text: probabilities as percent with one decimal, confidence, noul and score with two decimals,
  token counts equal to `usage` (R6.5).
- Keep pure logic (share codec, state-mode detection, number formatting, snippet generation) in plain Rust modules
  with no DOM access, so it is unit-testable and reusable by the snippet and share code.

## Share links
- Format: `#share/<compressToEncodedURIComponent(JSON)>`, payload
  `{"apiVersion":"v1","documentText","promptsText","selectedModels":["jev-latest"]}` exactly as Jev writes it.
- Encode with `lz_str::compress_to_encoded_uri_component(json_str)`. Decode with
  `lz_str::decompress_from_encoded_uri_component(s)`, which returns `Option<Vec<u16>>`; convert with
  `String::from_utf16` and treat `None`, invalid UTF-16 or bad JSON as a visible share-link error, never a panic.
- Read the hash on load and on `hashchange`; `location.hash` includes the leading `#`. `?autorun=1` runs the loaded
  share once after models load (R6.2 URLs).
- Round-trip test: the playground's own link must decode with lz-string 1.5.0 in Playwright (R7.4).

## Build: trunk, wasm-bindgen, wasm-opt
- `crates/ardana-playground/index.html` drives trunk. Every asset tag needs `data-trunk`:
  `<link data-trunk rel="rust" data-wasm-opt="z" />` for the crate and `<link data-trunk rel="css" href="styles/...css" />`
  per stylesheet. `data-wasm-opt` (`0`-`4`, `s`, `z`) only runs in `--release` builds.
- `Trunk.toml` sits next to `index.html` and pins tools under `[tools]`: `wasm_bindgen = "<Cargo.lock version>"` and
  `wasm_opt = "version_133"`. Trunk uses a tool from `PATH` only when its version matches, otherwise it downloads.
  Bump the pin, `xtask/fetch.toml` (`cargo-lock:wasm-bindgen`) and `cargo xtask fetch` together whenever Cargo.lock's
  wasm-bindgen changes.
- Build only with `cargo xtask build`, which runs `trunk build --release --offline` through `Sandbox::command`
  (`PATH` starts with `tmp/bin`, `HOME` under `tmp/`). `--offline` turns a missing or mismatched tool into an error
  instead of a download into `~/Library/Caches/dev.trunkrs.trunk`, which the home guard would flag.
- Never run trunk from a `build.rs` (outer cargo lock deadlock, rust-lang/cargo#8938) and never `cargo install trunk`
  into `~/.cargo/bin`; `trunk serve` is not part of the workflow, the playground is served by `ardana serve`.
- Keep `[build] filehash = true` (trunk's default) so hashed asset names let the server cache assets while
  `index.html` stays uncached.
- Do not set `opt-level = "z"` on the workspace `[profile.release]`: it would also shrink-optimize the server binary.
  Size work for the wasm goes through `data-wasm-opt`, dependency choices, and brotli from memory-serve.

## CSS and design
- Styles live only in plain `.css` files under `crates/ardana-playground/styles/`, linked with
  `<link data-trunk rel="css">`. No inline style strings, no CSS-in-Rust crates, no Tailwind: the /impeccable design
  hook scans `.css`, `.html`, `.ts`, `.js`, not `.rs`.
- Dynamic visuals (bar widths) set a CSS custom property or a class from the view; the rules that use them stay in
  `.css`. Class names come from DESIGN.md's tokens and components.
- Every UI change, from the first component on, goes through the /impeccable skill: PRODUCT.md, DESIGN.md and the
  `crates/ardana-playground` surface brief (Operate mode, six-block direction contract) exist before the first
  component; read the skill's `reference/craft-floor.md` before each UI edit; `buildPath` stays `"code"` with no image
  generation; the hook stays enabled (Q23, Q27).
- The Jev layout is a brand commitment: state on the left, questions and results on the right; it collapses to one
  column at 390 px width.
- Because markup is rendered from `view!` in `.rs`, file scans see nothing; verify rendered UI with
  `impeccable detect --viewport 1280x800` and `--viewport 390x844` against the running server URLs (R6.2), plus
  Playwright screenshots under `tmp/screens/<case>/`.

## Testing
- Unit-test the pure modules with `cargo test -p ardana-playground` where they do not touch the DOM.
- Behavioural checks are Playwright suites in `e2e/playground` (`cargo xtask e2e playground`) against the embedded
  binary serving decider-2b from `tmp/hf`; never replace the model with canned responses in e2e.

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
- https://github.com/trunk-rs/trunk/blob/v0.21.14/guide/src/assets/index.md — `data-trunk`, `rel="rust"`, `data-wasm-opt` release-only, `rel="css"`
- https://github.com/trunk-rs/trunk/blob/main/guide/src/configuration/index.md — config layering, `[tools]` PATH matching, offline errors
- https://github.com/trunk-rs/trunk/blob/v0.21.14/Trunk.toml — `[build] offline`, `filehash`, `minify`, `[tools]` keys
- https://github.com/trunk-rs/trunk/blob/main/guide/src/commands/index.md — `trunk build`, `trunk tools show`
- https://github.com/trunk-rs/trunk/releases/tag/v0.21.14 — 0.21.14 release notes
- https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/usage.html — CLI version must equal the crate version
- https://github.com/WebAssembly/binaryen/releases/tag/version_133 — binaryen `version_133` release
- https://docs.rs/lz-str/0.2 — `compress_to_encoded_uri_component`, `decompress_from_encoded_uri_component`, UTF-16 output
- https://developer.mozilla.org/en-US/docs/Web/API/Window/hashchange_event — `hashchange`, `location.hash` with `#`
- https://developer.mozilla.org/en-US/docs/Web/API/Performance/now — monotonic request timing
