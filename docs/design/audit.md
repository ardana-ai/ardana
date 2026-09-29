# Playground audit

Impeccable `audit` of `crates/ardana-playground` in its Notion-language redesign, run by a fresh-context agent against
a live `ardana serve` (decider-2b) on the installed Chrome through Playwright: 1280x800, 390x844, 320 wide, 200% page
zoom, 200% text-only resize, reduced motion, forced colours (light and dark), `prefers-color-scheme: dark`; the
empty, loaded, results, 422, stale, builder-open, invalid-JSON, popover, toast, tooltip and busy states; computed
contrast of every text node and component boundary against its composited background in both schemes; a
keyboard-only pass (Tab order, skip link, presets and Restore previous from the sidebar and the phone drawer, type
switch by arrow keys and back, Edit, rename, Add and Remove option and level at both bounds, Remove question,
Ctrl+Enter, Copy, the Share popover, Ctrl+\, the drawer); live-region mutations for a run, a 422 twice, a JSON error
and its recovery; the `#run-note` banner; keystroke latency at 40 KB and 400 KB of state; network sizes and paint
timings; `impeccable detect --json` on `index.html`, `styles/` and the four live states at both viewports. Evidence
(scripts, JSON, notes, screenshots) lands in `tmp/evals/audit/`. Not exercised: real screen-reader output (live
regions were observed as DOM mutations), touch gestures (the page has no drag surfaces), Safari and Firefox.

P0: 0 · P1: 0
P2: 4 · P3: 8

## Audit Health Score

| # | Dimension | Score | Key finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | 3 | Rename by Tab, the identical-422 announcement, focus after a phone run and the field edges remain (P2) |
| 2 | Performance | 3 | 296 KB wasm (Brotli), FCP 104–116 ms, 16 ms per keystroke at 40 KB; 115 ms at 400 KB (three state parses) |
| 3 | Responsive design | 3 | Nothing clips at 320 px, 200% zoom or 200% text; touch targets 24–28 px (WCAG 2.5.8 minimum, not 44) |
| 4 | Theming (token discipline) | 4 | Every text pair ≥ 4.55:1 in both schemes; the picked segment's ring ≥ 3.05:1; no dead tokens |
| 5 | Implementation integrity | 4 | Coherent and intentional; `impeccable detect` 0 findings on the sources and the live states; every glyph drawn |
| | Total | 17/20 | Good |

Implementation integrity verdict: pass. The page is one product: a workspace sidebar, one page of blocks, hairlines,
one blue, property rows and toggles, with every figure carrying `data-field`/`data-value` back to the response.

## Round history

- Round 1 (after the first build): P0 0 · P1 3 · P2 8 · P3 12, 15/20. The P1s were: the top bar clipped Run and
  Share off a 320 px page (and a phone at 200% text) while the stale tag showed; the CSS tooltips were neither
  hoverable nor dismissible (WCAG 1.4.13); a pick in the phone drawer hid the focused row and dropped focus to
  `<body>`. Fixed together with the P2s the batch reached: the page is `inert` behind the drawer, the Close-sidebar
  tooltip hangs inside the sidebar, the picked segment carries a 3:1 ring.
- Round 2: P0 0 · P1 1 · P2 6 · P3 13. The stale tag now gives way (and hides below 720 px), tooltips are hoverable
  through a transparent 6 px border and dismissed by Escape (`.tips-off` on the root), a preset picked in the drawer
  focuses the state; the section jumps still focused into the inert page. Fixed by settling the drawer first and
  focusing the target on the next frame; also the `link` glyph, the unused icons and classes, `h3` captions under
  the toggle headings, `role="group"` on the wire panes, forced-colour rules for held buttons and the winning bar,
  the busy label at full opacity.
- Round 3: P0 0 · P1 1 · P2 5 · P3 8, 17/20. "Raw exchange" and "Snippets" jumps land focus on their summaries under
  the top bar; "State and questions" focused Raw exchange's summary instead of the page, because the jump looked for
  any `summary` under `#content`. Fixed: only a `details` hands focus to its own summary, the page takes it itself;
  verified by Playwright at 390 px (focus on `main#content` at the top of the view, the drawer closed, the page no
  longer inert).

## Open findings

### P2
1. A repeated status is not re-announced: two identical 422s in a row mutate `run-status` once (`src/ui/mod.rs`,
   `run_status`); the text is what the e2e suite asserts, so a run counter cannot be added to it. Clear the region
   for one frame before refilling it.
2. On a phone a run scrolls the first question to the top while focus stays on Run (`src/ui/mod.rs`,
   `scroll_to_result`); `answers_match_api` asserts Run keeps focus on both projects, so the scroll would have to
   move focus only when the columns stack and the suite accept it.
3. Form field boundaries sit under 3:1 (`styles/controls.css` `.field-input`, `.select-input`; `styles/page.css`
   `textarea.code-block`): Notion's own 16% hairline. Fields are found by their labels and placeholders; raising
   `--hairline-strong` toward 40% ink would meet WCAG 1.4.11 at the cost of the reference look.
4. Typing in a 400 KB state costs 115 ms per keystroke: the state is parsed as JSON three times per `input`
   (`src/ui/state.rs`, `src/deck.rs` `inputs_changed`, the snippet memo). Parse once into a shared `Memo<Value>`
   and compute the snippet lazily.

Closed after round 3: a rename committed by Tab now lands focus on the Instructions field under the new id (a Tab
keydown on the id field is the signal, since `change` fires before focus moves); Enter keeps it on the id field.
Verified by Playwright at both viewports and by `builder_sync`.

### P3
6. `bar-grow` animates `width` and the shell animates `grid-template-columns` (10 bars, 200 ms); `transform` would be
   cheaper.
7. `aria-controls` names ids that do not exist while closed (`share-panel`, `{dom}-program`).
8. Every property name is a `dfn tabindex="0"`: four extra tab stops per answered question, each carrying its
   meaning in a tooltip and hidden text.
9. Loading a preset is not announced; Add option and Add level leave focus on the Add button.
10. The Share popover stays open when focus tabs out of it; the Run tooltip draws over it (both `z-index: 30`).
11. Touch targets are 24–28 px on the phone; WCAG 2.5.8 passes at its 24 px minimum.
12. A literal shadow or two beside the tokenised ones (`controls.css` the toast, `page.css` the segment lift's second
    layer).
13. With the sidebar collapsed the workspace name leaves the tree (it is plain text now; the page's `h1` is the page
    title, so the outline holds).

## Positive findings

- Every text pair measures ≥ 4.55:1 in light and ≥ 4.63:1 in dark; secondary `ink-2` reads at 5.3–6.3:1; the focus
  ring holds 3.6–4.5:1 on every surface; Run's white label 4.68:1 on `#0075d3`; the dark fault ink `#f28b88` 6.16:1
  on its tint; the picked segment's ring 3.05:1 light and 3.96:1 dark.
- Reduced motion removes every animation and transition (bar growth, spinner, drawer, shell, toggle marker) and
  the scroll goes instant; the "Running" label carries the busy state without the spinner.
- Forced colours: buttons, fields, tags, blocks, code, the banner and the sidebar edge get `CanvasText` borders,
  held buttons `GrayText`, the picked segment a 2 px `Highlight` outline, the winning bar and mark
  `Highlight`/`HighlightText`, the other bars `GrayText`.
- The keyboard pass completes with no trap: skip link → `#content`; the type control is a native radio group; Add
  and Remove use `aria-disabled` and keep focus at both bounds; Remove question moves focus to the next Edit;
  Restore previous focuses State; Escape closes the popover, the drawer and every tooltip and returns focus; the
  page is `inert` behind the drawer; a drawer pick lands focus on the state or the jumped section.
- Live regions announce a run, a 422, a JSON error and its recovery, and the copy toast; the `#run-note` banner
  names why Run is held ("Add a question to run", "Questions JSON has an error") and describes it.
- No horizontal scroll at 320 px, 200% zoom or 200% text; `scroll-margin-top` keeps scrolled targets clear of the
  sticky bar; the popover is fixed and fully on screen under 720 px.
- 296 KB wasm (Brotli), 8 KB CSS, no web fonts; first paint 44–116 ms; models listed at ≈ 118 ms.
- Errors show the API's `detail` verbatim, the 422 marks the named question, every figure carries `data-field` and
  `data-value`, and stale answers stay visible while marked on the block, in the last-run line and in the top bar.
