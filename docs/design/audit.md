# Playground audit

Impeccable `audit` of `crates/ardana-playground` in its ardana.ai brand world (the "In browser" rows that run a library
model's browser variant in the tab, and the ardana CLI handoff for models the server lacks) and of
`crates/ardana-server/placeholder/`, round 6: a fresh-context agent re-verified round 5's P1 after its fix and
regression-checked what the fix touched, against the release build of the working tree (commit ef50236 and the
uncommitted fix: `ui/topbar.rs#follow_heights` keeps `--topbar-height` and `--banner-height` on the root from a
`ResizeObserver` on the top bar and the banner; `styles/base.css` and `styles/shell.css` read them for the scroll
padding, the in-flight banner's sticky `top` and the phone Share popover's `top`; `ui/mod.rs#show_text_field` brings an
editor that Tab reaches whole into view; the binary of 00:52 on 2026-10-03, its served wasm, JS, `engine.js` and
stylesheets byte-identical to `dist/` and the sources) behind a local server (`ardana serve`, decider-2b pulled) and a
public one (`ardana serve --public`), on the installed Chrome 154 through Playwright with persistent profiles. Round 5's
walk (a server run's answers on the page while decider-0.8b downloads into the tab; Shift+Tab and Tab through every
control and fact term: round 5's 16-stop walk, whole cycles, and whole cycles with a question's builder open) at
320x640, 360x740, 390x844, 430x932 and 1280x800, 100% and 200% text (round 5's root font-size rule), light and dark,
2,832 stops; the same walk in landscape (844x390, 932x430) and at 200% and 400% zoom (640x400, 320x256); round 5's own
focus and banner scripts re-run unchanged; every stage of a run in the tab held (a proxy holding the server's profile
for Pulling and onnxruntime-web's WASM module for Starting, the download throttled; Running recorded as it passed) at
300 to 1280 px with both text sizes, the bars' boxes checked against the measured properties at every change of the
banner, Stop hit-tested, the root's style changes and window errors counted; the top bar at 17 widths from 280 to 1280
px with both text sizes; the phone Share popover at seven widths with both, at rest and in flight; the skip link, the
sidebar's "On this page" rows and "Show the command"; Tab and clicks into the editors; reduced motion; the public
server's opening page and an in-tab run on it; round 5's carried-findings script re-run; `impeccable detect --json` on
five live states (local empty, loaded, results and run-command; the public opening page) at 1280x800 and 390x844 in
light and dark, and on `index.html`, `styles/` and `placeholder/`. Evidence (scripts, JSON, notes, screenshots): round 1
in `tmp/evals/audit/`, rounds 2 to 6 in `tmp/evals/audit/round2/` to `round6/`, each indexed by its `notes.txt`. Not
exercised this round: a real screen reader (the fix changes no name, role or live region), real phones and their
collapsing toolbars (the viewports were emulated; the popover was opened by a pointer click in a touch-enabled context,
not a tap), Safari, Firefox, Windows and Linux (the `ResizeObserver` and `:has()` paths ran in Chrome only),
operating-system text scaling (Android's font scale, iOS Dynamic Type), a real pull from the Hub past the 400 ms grace
(Pulling was held by the proxy), the Running stage held (it passed in 194–577 ms on WebGPU; WASM was not run), and
contrast, forced colours, the cold load and the tab's memory (the fix adds no colour and 23 KB of raw wasm; round 5's
measures stand); no end-to-end suite was run.

P0: 0 · P1: 0
P2: 8 · P3: 12

Since round 6, every finding below was fixed in `42ecb01..4576fb7`, each proven by a Playwright case or a measurement,
not by a new audit round (none was run): 1 `banner_reflow`; 2 `text_reflow`; 3 and 14 `skip_link`; 4 `announcements`; 5
`run_focus`; 6 a key in a 400 KB state takes 24 ms at the median with Snippets open (was 120–128 ms;
`tmp/evals/u2/typing-*.json`); 7 `browser_resume` (the weights cross the wire about once); 8 the page's wasm is 375 KB
brotli and the first paint 2.7 s at 1.6 Mbps (was 935 KB and 5.4 s; `tmp/evals/engine/load-final.json`), the engine a
module of its own (`crates/ardana-engine`) imported on an "In browser" pick; 9 the bars and the shell animate
`transform` only, or nothing (`tmp/evals/u2/motion-*.json`); 10 `aria_relationships`; 11 `term_stops`; 12
`announcements` and `page_focus`; 13 `share_popover`; 15 `page_weight`; 16 `touch_targets`; 17 `current_row`; 18
`browser_pull` and `browser_recover`; 19 a server pick frees the GPU process's 625–1,395 MB, and the banner says running
takes 3 to 4 times the download in memory (`tmp/evals/u2/memory*.json`); 20 `term_tooltips`.

## Audit Health Score

| # | Dimension | Score | Key finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | 3 | Round 5's P1 is closed: no focus stop hidden under the bars in 2,172 page stops at 320–1280 px, 100% and 200% text, while a download is in flight; open: "Skip to the page" changes the URL so that Back reloads the share link over the edits, a repeated 422 is silent, a phone run leaves focus behind |
| 2 | Performance | 2 | Unchanged by the fix (the wasm 914 KB brotli, 23 KB more raw): the first paint waits for the in-tab engine's wasm, an interrupted 447 MB weights download restarts from zero, a kept session holds 583 MB |
| 3 | Responsive design | 3 | Every offset now follows the measured bar and banner at every width and text size, but in flight the bars hold 160 of 256 px at 400% zoom and 361 of 640 px with 200% text, and with 200% text the Snippets block runs 476 px wide off a phone's screen; touch targets 24–40 px |
| 4 | Theming (token discipline) | 4 | The fix adds no colour; every colour a `:root` token with a dark twin; round 5's measures stand (no text pair under AA, min 5.09:1 light, 5.62:1 dark; the download bar's fill 3.12:1 and 3.53:1 on its track) |
| 5 | Implementation integrity | 4 | The measured heights live in two root properties that only the stylesheets read, as DESIGN.md and the guideline say; the observer writes only when a box changes size; `detect` reports 0 findings on all 20 live scans and the file scans |
| | Total | 16/20 | Good |

Implementation integrity verdict: pass. The fix stays inside the system: the top bar's and the banner's rendered heights
are two custom properties on the root (`--topbar-height` and `--banner-height`, defaulting to `--topbar` and 0), written
by one `ResizeObserver` and read only by the stylesheets (the scroll padding, the in-flight banner's `top`, the phone
popover's `top`), while the nominal `--topbar` keeps only the bar's and the sidebar head's `min-height`, which is what
DESIGN.md's shell paragraph and banner entry and `docs/guidelines/leptos.md` now say. Round 5's wrong comment ("56px is
the banner with its bar") is gone with the constant it described. Over a whole run the observer wrote the root's style 3
to 6 times, only when a box changed size and never at rest; no run raised a window error or a ResizeObserver warning
(the console carried only Chrome's notice that the wasm preload's `integrity` is ignored, trunk's markup as before). No
colour, glyph or string was added. `impeccable detect` reports 0 findings with the project config on `index.html`,
`styles/`, `placeholder/` and all 20 live scans (five states, two viewports, light and dark); without the config it
reports only the waived Geist Mono (`overused-font`, six faces in `fonts.css`, one in `placeholder.css`), as in round 5.
One promise of DESIGN.md's banner entry holds only up to a size: focus "scrolls clear of both, however many lines the
bar and the banner take" until the bars outgrow the viewport (400% zoom with 200% text; finding 1).

## Executive summary

- Audit Health Score 16/20 (Good), with no P0 and no P1 open: round 5's P1, focus hidden under the phone banner, is
  closed.
- 20 findings: P0 0, P1 0, P2 8, P3 12.
- The closure, on round 5's own walk: with a server run's answers on the page while the tab downloads, Shift+Tab put
  Certainty and P max at y=172–189 at 390 px, 12 px under a banner ending at 160 (round 5: 142–159, hidden); across 20
  configurations (320, 360, 390, 430 and 1280 px; 100% and 200% text; light and dark) 2,172 page stops showed whole or,
  taller than the room left, filled it, none hidden and no ring cut, all 480 fact-term stops and 240 editor stops whole.
  Round 5's own scripts, re-run unchanged, find no term hidden at 320–720 px, nothing hidden in the 200%-text walks, and
  the banner 0 px under the bar at all 13 sizes.
- What the fix touched holds: through every stage of a run in the tab (the size line before Run, Pulling, Downloading,
  Starting, Running, the answers) the properties equal the boxes, and while the run is in flight the banner sticks right
  under the bar (also when the bar wraps mid-run at 300 px, 70 → 104 px) with Stop hit-testable; the popover hangs 8 px
  under the bar at every phone width and text size (over Run by 0 px; round 5: 17 px); the Raw exchange and Snippets
  jumps and "Show the command" land 11.5–12.5 px under the bars, and the skip link and "State and questions" bring the
  page to its top; a click into an editor scrolls nothing and Tab brings it whole; reduced motion and the public
  server's page hold.
- New: at 400% zoom or with enlarged text the in-flight bars take most of a small screen (P2); "Skip to the page"
  changes the URL, and Back then reloads the share link over the edits (P2); a fact term's tooltip opens over the
  in-flight banner (P3). Finding 2 is narrowed to the segmented rows that do not wrap with enlarged text, now measured
  pushing the snippet's copy key wholly off a phone's screen.
- Top issues: the in-flight bars crowding small screens; the skip link's fragment; the enlarged-text Snippets block; a
  cold first load that carries the in-tab engine; the weights restarting after any interruption; a silent repeated 422;
  a phone run leaving focus behind; typing lag at 400 KB.
- Next: `$impeccable adapt` for a compact in-flight banner and wrapping segmented rows, `$impeccable harden` for the
  skip link, announcements and focus, `$impeccable optimize` for the wasm, resumable downloads and the snippet, then
  `$impeccable polish`.

## Findings

### P1

None open. Round 5's P1 (on phones the in-flight banner hid keyboard focus, WCAG 2.4.11) is closed; its evidence is in
the round history (round 6).

### P2

1. **While a run is in flight, the sticky bars can take most of a small or zoomed screen** (new)
   - Location: `styles/shell.css:303-313` (the whole banner sticks at `top: var(--topbar-height)`, however tall its
     words make it, and the scroll padding adds all of it), `styles/shell.css:477-489` (at 720 px and below the words
     take their lines and the bar and Stop the next), `src/ui/mod.rs:385-412` (`show_text_field` leaves an editor taller
     than the room below the bars where Chrome puts it).
   - Category: Responsive (Accessibility).
   - Evidence: At 320x256 (WCAG 1.4.10's 1280x1024 at 400% zoom) the bar (70 px) and the in-flight banner (90 px) hold
     160 of 256 px for the whole download, leaving 84 px under the scroll padding, where the editors (200–220 px) show
     81–82 px of themselves (`round6/p1-zoom-320.json`). With 200% text they hold 361 of 640 px at 320x640 (bar 113 px;
     banner 248 px: four lines of words, the bar, then Stop on a row of its own;
     `round6/screens/st-320-text200-downloading-scrolled.png`), 319 of 740 at 360 px, and 305 of 844 and of 932 at 390
     and 430 px; in landscape with 200% text 226 px of 390 (844x390) and of 430 (932x430), where the editors show 67–175
     px of their 200–220 and the questions JSON keeps only its closing brace in view
     (`round6/p1-landscape-summary.json`, `round6/landscape-caret.json`,
     `round6/screens/lc-932x430-text200-questions.png`). With 400% zoom and 200% text together the bars (361 px) outgrow
     the 256 px viewport: every page stop is hidden and Stop (311–353 px) lies below the viewport
     (`round6/p1-zoom-320.json`). Focus is never wholly hidden at 400% zoom or with 200% text alone (0 of 2,172 page
     stops in the P1 walk, 0 in landscape and at zoom), and at default text the bars are moderate (160 of 640–932 px on
     phones, 110 of 800 at 1280). Before the fix the banner stuck at 70 px under a 113 px bar, so with 200% text the
     sticky region was 43 px smaller, but the bar hid 43 px of the banner, the first line of its words included.
   - Impact: For the length of a download, minutes on a phone, people who zoom to 400% or enlarge text read and edit the
     page through a slot of 84–267 px.
   - Standard: WCAG 1.4.10 Reflow is met by its letter at 320x256 (no sideways scroll, nothing lost) and 2.4.11 holds
     (focus never wholly hidden); 2.4.12 Focus Not Obscured (Enhanced, AAA) fails for the editors in landscape; as
     practice, sticky regions leave most of the viewport to the content at high zoom.
   - Recommendation: Keep the in-flight banner to one row at every width and text size (the count shortened to "2.3 of
     467.7 MB", the bar and Stop on the same row), or stick only the bar and Stop and let the words scroll away; with
     the measured heights in hand, stop sticking the banner when the bars would pass about a third of the viewport
     (`--topbar-height` and `--banner-height` against `100dvh`).
   - Suggested command: `$impeccable adapt`

2. **With enlarged text the segmented rows do not wrap, and the Snippets block runs off a phone's screen** (round-3 P2,
   narrowed in rounds 4 and 6)
   - Location: `styles/controls.css:127-131` and `:140-143` (`.segmented` and `.segmented-options` are inline-flex rows
     that never wrap), against `.main`'s `overflow-x: clip` (`styles/shell.css:181-187`).
   - Category: Responsive (Accessibility).
   - Evidence: With 200% text and answers on the page the Snippets block (its Language segments and copy key) is 476 px
     wide at every width up to 480 px. At 280–430 px the snippet's copy key (446–487 px) lies wholly outside the
     viewport, clipped, so Tab focuses it out of sight and only part of its tooltip shows at the edge
     (`round6/screens/ov-390-text200-copy-focused.png`); the last language segment is cut, and the curl snippet's lines
     lose their last 63–212 px (`round6/overflow-text200.json`). At 280–360 px the type segments run to 366 px and the
     question block, its builder fields and the questions JSON run with them (to 362–378 px), and at 280–340 px the page
     scrolls sideways by 2–62 px (`round6/overflow-text200.json`, `round6/p1-walk.json`). At 200% and 400% zoom with
     default text nothing runs past the edge (`round6/p1-zoom-640.json`, `round6/p1-zoom-320.json`).
   - Narrowed this round: the bar still wraps with enlarged text (113 px at 280 and 390–415 px, 73 px elsewhere, 126 px
     at 844 and 932 px during a run) and mid-run at 280–305 px (104 px), but what assumed a one-line bar now reads its
     measured height: whole Tab and Shift+Tab cycles at rest with 200% text at 280–430 and 1280 px hide nothing
     (`round6/regress-bar.json`), the phone popover hangs 8 px under the 113 px bar and overlaps Run by 0 px (round 5:
     17 px; `round6/carried.json`), the banner sticks under the wrapped bar with 0 px covered (round 5: 34–43 px;
     `round6/banner-geometry.json`), and round 5's 200%-text Shift+Tab walk hides nothing (round 5: Snippets' summary
     and the questions editor wholly hidden; `round6/focus-hidden.json`).
   - Impact: People who enlarge text on a phone cannot see the snippet's copy key when it has focus, nor the end of the
     curl snippet's lines; on the narrowest phones the page also scrolls sideways.
   - WCAG: 1.4.4 and 1.4.10 are met through browser zoom; under text-size settings, 2.4.7 Focus Visible for the copy key
     and 1.4.4 for the snippet's text, as practice.
   - Recommendation: Let `.segmented` and `.segmented-options` wrap, and let the snippet's head put the copy key on its
     own row when the segments wrap.
   - Suggested command: `$impeccable adapt`

3. **"Skip to the page" changes the URL, and Back then reloads the share link over the edits** (new)
   - Location: `src/ui/mod.rs:217` (`<a class="skip-link" href="#content">`), `src/ui/mod.rs:108-117` (every
     `hashchange` loads the fragment's share link), `src/deck.rs:445-462` (`load_share` replaces both editors and the
     model pick, keeping nothing for Restore previous).
   - Category: Accessibility (Implementation integrity).
   - Evidence: Opened from a share link, the state edited, then Enter on "Skip to the page": the address bar reads
     `/#content` (a new history entry, `history.length` 3) and no longer holds the share link; the browser's Back
     returns to `#share/…`, whose `hashchange` loads the link again: the edit is gone and no Restore previous is offered
     (`round6/misc.json` skipBack). The sidebar's "State and questions" row, the same jump through `reveal("content")`,
     leaves the URL as it is (`round6/regress-jumps.json`).
   - Impact: Keyboard users who skip to the page and later press Back, or swipe back by accident, stay on the page and
     silently lose what they changed in the editors (and the model pick, which `load_share` resets too), with no undo;
     the address they copy or bookmark no longer holds the share link.
   - Recommendation: Let the skip link focus and scroll without navigating (handle its click: `preventDefault`, then
     `reveal("content")`), or replace the history entry instead of pushing one; let `load_share` keep the editors for
     Restore previous when they differ from the link.
   - Suggested command: `$impeccable harden`

4. **A repeated outcome is not re-announced** (carried over)
   - Location: `src/ui/mod.rs:218-220` (`run-status`), `src/ui/mod.rs:286-318` (`run_status`).
   - Evidence: The same invalid request run twice: 0 mutations of either status region the second time
     (`round6/carried.json` repeated422, as in round 5).
   - Impact: Screen-reader users hear nothing after pressing Run again on the same error.
   - WCAG: 4.1.3 Status Messages (AA), in spirit: the message exists but is not sent again.
   - Recommendation: Empty the region for one frame before refilling it.
   - Suggested command: `$impeccable harden`

5. **On a phone a run scrolls to the answers while focus stays where it was** (carried over)
   - Location: `src/ui/mod.rs:206-211`, `src/ui/mod.rs:371-383` (`scroll_to_result`).
   - Evidence: From the state editor at 390 px (Ctrl+Enter) the run scrolls the first question to y=82, 12 px under the
     bar, with focus still in the editor, now off screen above (`round6/carried.json` phoneRun); round 5's tapped runs
     (`round5/tp-public-phone.json`) ran on the same code: Run kept focus while the fault or the answers scrolled into
     view, and Run's next Tab stop is the state editor, at the top.
   - Impact: Keyboard and switch users on a phone lose the answers they were just shown.
   - Standard: WCAG 2.4.3 Focus Order (as practice).
   - Recommendation: Move focus to the fault or the first question when the columns stack.
   - Suggested command: `$impeccable harden`

6. **Typing in a 400 KB state costs about 120–128 ms per key** (carried over)
   - Location: `src/ui/snippets.rs:17` (the snippet memo over `deck.request()`), `src/ui/snippets.rs:67` (the ardana
     command's memo, the same work for a model the server does not run).
   - Evidence: Ten keystrokes in a 400 KB state with Snippets open: median 120 ms with curl and 128 ms with the ardana
     command, max 160 ms, a long task of 111–128 ms on every key (`round6/carried.json`; round 5: 128 ms, max 152–160
     ms).
   - Impact: Long transcripts type sluggishly; the workaround is to collapse Snippets.
   - Standard: Interaction to Next Paint (good ≤ 200 ms; here 120–128 ms at the median, up to 160 ms).
   - Recommendation: Build and render the snippet only while Snippets is open, debounced (about 150 ms) above a size
     threshold.
   - Suggested command: `$impeccable optimize`

7. **An interrupted or stopped download of the weights starts over** (carried over)
   - Location: `crates/ardana-server/src/browser.rs:62-104` (`serve` sends the whole file, without `Accept-Ranges` or
     `Range`), `src/api.rs:172-232` (`browser_file` reads one whole response), `src/engine.rs:527-573` (`Files::get`
     keeps a file only once it is whole).
   - Category: Performance.
   - Evidence: Round 5's, on code this round does not touch: a stream severed 125 MB into `model.onnx.data` (145.6 MB of
     467.7 MB) left the tokenizer and the graph kept, and Run again downloaded all 447,348,736 bytes of the weights
     again (`round5/tab-drop.json` network); a request with `Range: bytes=0-15` gets `200` and the full length. The
     fault says "Run again once the connection is back", not that the weights start over; a Stop at 90% costs the same.
   - Impact: On a phone, each dropped connection or Stop during the weights costs up to 447 MB more data and minutes
     more waiting.
   - Recommendation: Serve `/v1/browser/*` with byte ranges (`Accept-Ranges: bytes`, `206`), keep the downloaded prefix
     (in chunks, in Cache Storage or IndexedDB), and resume with `Range: bytes=<had>-` and `If-Range` on the ETag; until
     then, say after a fault and after Stop that the weights start over.
   - Suggested command: `$impeccable optimize`

8. **First paint waits for a 914 KB wasm that carries the in-tab engine** (carried over)
   - Location: `crates/ardana-playground/Cargo.toml:12` (`ardana-core` in the playground's one wasm, for
     `src/engine.rs`), `index.html` (client-rendered: the page is blank until the wasm runs).
   - Category: Performance.
   - Evidence: This build's wasm is 935,577 bytes brotli and 3,730,258 raw (round 5: 3,707,220 raw; the fix's code is
     the difference). Round 5's measures stand: a cold first load of 1,048 KB on the local server and 1,081 KB on the
     public one, first contentful paint 87–116 ms on loopback and 5,435–5,447 ms at 1.6 Mbps with 150 ms of latency,
     blank until then (`round5/load.json`; round 4: 451 KB in all, the wasm 316 KB). onnxruntime-web is loaded only on
     an "In browser" pick; the tokenizer and planner are not, so every visitor pays for them, including those who only
     run server models.
   - Impact: The public demo's first visit on a slow phone connection shows nothing for about three seconds longer than
     before the in-tab engine.
   - Recommendation: Build the in-tab engine (ardana-core's tokenizer and planner) as a second wasm module imported on
     the "In browser" pick, as onnxruntime-web is; or prerender the shell so the first paint does not wait for the wasm.
   - Suggested command: `$impeccable optimize`

### P3

9. **Bars and the shell animate layout properties** (carried over). Location: `styles/page.css:309,318-322` (`bar-grow`
   animates `width`, 600 ms; the served keyframes read `0% { width: 0px; }`), `styles/shell.css:9`
   (`grid-template-columns`, 200 ms). Category: Performance. Impact: layout work for every bar of every run and on every
   collapse (`round6/carried.json` barGrow, shellTransition). Standard: compositor-only animation. Recommendation: grow
   the fill with `transform: scaleX()` from the left. Suggested command: `$impeccable optimize`.
10. **`aria-controls` and `aria-describedby` name ids that exist only sometimes** (carried over). Location:
    `src/ui/topbar.rs:344` (`share-panel`), `src/ui/questions.rs:337` (`{dom}-program`), `src/ui/topbar.rs:197` (Run's
    `aria-describedby="run-note-text"`, absent whenever the banner has nothing to say). Category: Accessibility. Impact:
    three dangling relationships while closed, and Run's on a page with a pulled model and questions
    (`round6/carried.json` ariaControls, runDescribedBy). Standard: ARIA 1.2. Recommendation: render the panels hidden,
    or set the attributes only while their targets exist. Suggested command: `$impeccable harden`.
11. **Fact terms and tooltips over-announce** (carried over). Location: `src/ui/questions.rs:439`,
    `styles/controls.css:316` (`content: attr(data-tip)`). Category: Accessibility. Impact: every fact name is a `dfn
    tabindex="0"`, and a shown tooltip joins the accessible name: after a run in the tab, and after its fault, focused
    Run is named "Run Send the state and questions Ctrl+Enter or ⌘↵" in Chrome's tree (`round5/tab-desktop.json`
    axAfter, `round5/tab-drop.json`; code unchanged). Standard: accname. Recommendation: `content: attr(data-tip) / ""`,
    as the curl prompt already does; one term stop per question. Suggested command: `$impeccable harden`.
12. **Focus and announcements after page-driven changes leave users behind** (carried over). Location:
    `src/ui/sidebar.rs:74-93` (presets), `src/ui/builder.rs:190-215,360-367` (Add option and Add level),
    `src/ui/topbar.rs:273-285` (Stop is rendered only while the download is stoppable). Category: Accessibility. Impact:
    loading a preset announces nothing (0 mutations, `round6/carried.json`); Add keeps focus on the button the new row
    pushes down (round 3, code unchanged); focus parked on Stop drops to `<body>` when the download ends and Stop
    leaves, though Chrome's next Tab still continues to the state editor (`round5/focus-stop.json`, code unchanged).
    Standard: WCAG 4.1.3 and 2.4.3 as practice. Recommendation: announce "Loaded <preset>"; focus the new row's first
    field; when Stop leaves while focused, move focus to Run, as Stop's own press does. Suggested command: `$impeccable
    harden`.
13. **The Share popover outlives its focus** (carried over). Location: `src/ui/topbar.rs:291-377`,
    `styles/controls.css:322` and `styles/shell.css:392` (both `z-index: 30`). Category: Accessibility. Impact: Tab out
    of the popover leaves it open, and Escape no longer closes it once focus is outside (`round6/carried.json`
    sharePopover). Recommendation: close on `focusout` beyond `#share` and handle Escape at the window while open.
    Suggested command: `$impeccable harden`.
14. **The skip link stays reachable under the open drawer** (carried over). Location: `src/ui/mod.rs:217,227` (only
    `.main` is `inert`). Category: Accessibility. Impact: Tab from the drawer's last row reaches "Skip to the page"
    (`round6/carried.json` skipUnderDrawer), which targets the inert page. Recommendation: make the skip link inert
    while the drawer is open, or let it close the drawer first (with finding 3's change, through `reveal`). Suggested
    command: `$impeccable harden`.
15. **Two glyphs and a doubled logo add weight** (carried over). Location: `src/ui/sidebar.rs:129` and
    `src/ui/topbar.rs:170,198` (⌘ and ↵), `styles/fonts.css:25-63` (Onest's math and symbols subsets),
    `src/ui/logo.rs:12`, `src/ui/sidebar.rs:56`, `src/ui/topbar.rs:175`. Category: Performance. Impact: the math and
    symbols subsets are 43.2 KB of every first load for two characters (`round5/load.json`, code unchanged), and the
    lockup is parsed into the DOM twice. Recommendation: draw the shortcut glyphs as SVG (or spell them out); render the
    lockup once as a `<symbol>`. Suggested command: `$impeccable optimize`.
16. **Touch targets are 24–40 px** (carried over). Location: `styles/controls.css:113-125` (`.button-sm` and small icon
    buttons at 28 px), `styles/controls.css:145-149` (segments 28 px tall), `styles/shell.css:317-329` (banner actions
    at a 24 px minimum). Category: Responsive. Impact: on a phone, Stop, the direct way to end a 467.7 MB download, is
    32x24 px at the end of the bar's row, and "Show the command" is 153x24 px (`round5/tp-public-phone.json`,
    `round5/show-size.json`; code unchanged). Standard: WCAG 2.5.8 (AA) passes at its 24 px minimum; 2.5.5 (AAA) asks 44
    px. Recommendation: 44 px hit areas under `(pointer: coarse)`, visual size unchanged. Suggested command:
    `$impeccable adapt`.
17. **The current "On this page" row is told apart by lightness alone** (carried over). Location:
    `styles/shell.css:139-141`. Category: Accessibility (Theming). Impact: #ededed against #a1a1a1, both at 400, is
    2.21:1 in dark (`round6/carried.json` currentRow), and forced colours draw every row alike. Recommendation: a second
    cue that survives both schemes and forced colours, such as weight 500 or a 2 px ink rule at the row's start.
    Suggested command: `$impeccable polish`.
18. **Two residual edges of the in-tab run** (carried over). Location: `src/engine.rs:63-65,155-169` (Pulling after a
    fixed 400 ms), `src/engine.js:20-34` and `src/engine.rs:329-373` (a runtime build is imported again only when its
    import failed). Category: Implementation integrity. Impact: on a network with 600 ms of latency and every file kept,
    a run opens on "Pulling the browser files of decider-0.8b (467.7 MB) on the server" with Stop for 200 ms, though
    nothing is pulled (`round5/tab-drop.json` latency600); after the WebGPU runtime's WASM fetch failed, the retry
    answered on WASM in 2,049 ms (195 ms on WebGPU) and every later run stays on WASM until a reload, which nothing
    suggests (`round5/tp-public-phone.json`; code unchanged). Recommendation: say Pulling from the server's own signal
    (a header or a status the profile route sends while it pulls) rather than elapsed time; when a session fails because
    its runtime could not be fetched, forget the cached module so the next run imports it again under a new URL.
    Suggested command: `$impeccable harden`.
19. **A kept in-tab session holds about 583 MB of the tab's memory** (carried over). Location: `src/engine.rs:83-98`
    (`Loaded`, kept between runs), `src/engine.rs:318-324` (`replace` frees a session only for another browser model).
    Category: Performance. Impact: the page goes from 4 MB to 583 MB after a run in the tab on WebGPU and stays there
    after four forced garbage collections and a second run (`round5/memory.json`; code unchanged); WASM memory never
    shrinks, so only a reload returns it, and a phone is likely to discard such a tab in the background. Recommendation:
    release the session when a server row is picked, and say the memory cost beside the size line. Suggested command:
    `$impeccable optimize`.
20. **A fact term's tooltip opens over the in-flight banner** (new; what round 5's P1 leaves behind). Location:
    `styles/controls.css:315-338` (`.tip::after` opens above its control, `z-index: 30`), `styles/shell.css:303-313`
    (the banner sticks at `z-index: 19`, and the scroll padding puts focus 12 px under it), `src/ui/questions.rs:439`
    (the terms). Category: Accessibility (Responsive). Impact: a term that Shift+Tab brings up lands 12 px under the
    banner, and its tooltip (29–64 px tall at default text, 151 px with 200% text) opens above it, over the banner's
    foot: at 390 px it covers 70% of the download bar, at 430 px 10 px of the banner, at 1280 px 3% of Stop, and at 390
    px with 200% text 38% of the banner's words, 70–77% of the bar and 20% of Stop (`round6/misc.json` tips,
    `round6/screens/tip-390-text200-Certainty.png`); the progress shows again when focus moves or on Escape, and the
    status region keeps saying it. Recommendation: give the terms a `scroll-margin-top` the height of their tooltip, or
    open a term's tooltip downwards while the banner is sticky. Suggested command: `$impeccable polish`.

## Patterns and systemic issues

- Sticky geometry is measured now, but not bounded: every offset follows the bar's and the banner's rendered heights
  (round 5's pattern of constants is gone), yet the in-flight banner sticks whole however tall its words make it, so at
  400% zoom or with enlarged text it takes most of a small screen, and the tooltips that open upwards land on it
  (findings 1 and 20). A compact in-flight form, one row with the count, the bar and Stop, would settle both and leave
  the measurements as a safety net.
- Rows that never wrap: the segmented controls hold their width under enlarged text and carry the question block and the
  Snippets block past a phone's edge, where `.main` clips them (finding 2).
- The in-tab engine's costs land on everyone, or land again: its planner and tokenizer ride in every visitor's first
  load (finding 8), its weights restart after any interruption (finding 7), and its session holds 583 MB until a reload
  (finding 19).
- Status, focus and history after an action are still handled case by case: a repeated status, a preset, a new row, a
  phone run and Stop leaving the banner each leave the user to find out what happened, and the skip link's fragment
  turns the browser's Back into a silent reload of the share link (findings 3, 4, 5 and 12).

## Positive findings

- Focus is no longer hidden under the bars: in round 5's walk, with a server run's answers on the page while the tab
  downloads, 2,172 page stops at 320, 360, 390, 430 and 1280 px, 100% and 200% text, light and dark, show whole or fill
  the room left, all 480 fact-term stops and 240 editor stops whole, no ring cut (`round6/p1-walk-summary.json`); in
  landscape and at 200% and 400% zoom no stop is hidden.
- The measured heights follow every change and never loop: at each stage of a run in the tab (on phones 69–90 px of
  banner while pulling and downloading and 40–58 px while starting and running; 150–248 and 100 px with 200% text) the
  properties equal the boxes, the banner sticks right under the bar (also as the bar wraps mid-run and the download's
  words take a second line), Stop stays hit-testable, the root's style changes only when a box does (3–6 times a run,
  none at rest, none in 5 idle seconds mid-download), with no window error (`round6/stages.json`,
  `round6/regress-jumps.json`).
- Everything that reads the bar's height lands where DESIGN.md says: the Raw exchange and Snippets summaries 11.5–12.5
  px under the bar or the banner after a sidebar jump, also with reduced motion (instant, no transition left); "Show the
  command" 11.5–12.4 px under a 70, 73 or 113 px bar; the phone popover 8 px under the bar at 320–720 px with both text
  sizes, over Run by 0 px, over the banner in flight (`round6/regress-jumps.json`, `round6/extras.json`,
  `round6/regress-bar.json`).
- `show_text_field` is precise: a click into an editor half under the bars scrolls 0 px, and Tab into it brings it
  whole, 12 px under the bar or the banner (`round6/regress-jumps.json` textField).
- The public server's opening page holds: decider-0.8b's "In browser" row with its model note, "Add a question to run"
  in the banner, the bar's property right at every width and text size, and whole Tab cycles at rest and during an
  in-tab run hide nothing (`round6/public.json`).
- As round 5 measured them, on code this fix does not touch: the first run in the tab is guarded and controllable (the
  size line before Run at every width on both servers, autorun waiting, Stop by keyboard and touch, another pick
  stopping the download, completed files kept); each stage is announced once in the polite status region and Run is
  named Pulling, Loading or Running with the banner's words as its description; failures speak the page's words and
  recover without a reload; a kept run answers in about 1.2 s with only the 459-byte profile on the wire; the insecure
  origin is honest; the held Run sends nothing; no text pair under AA in 36 state and scheme pairs; reduced motion
  leaves nothing moving and forced colours draw the download bar in CanvasText with a Highlight fill.
- `impeccable detect`: 0 findings on 20 live scans and the file scans with the project config, light and dark.

## Recommended actions

1. **[P2] `$impeccable adapt`**: a compact in-flight banner (one row with the count, the bar and Stop; or only the bar
   and Stop sticky; unstuck past about a third of the viewport); let the segmented rows wrap so the question block and
   the Snippets block fit enlarged text; re-run `round6/p1-zoom.mjs`, `round6/p1-landscape.mjs` and
   `round6/overflow.mjs`.
2. **[P2] `$impeccable harden`**: the skip link without a fragment (through `reveal`), `load_share` keeping the editors
   for Restore previous; re-announce a repeated status; focus the answers after a phone run.
3. **[P2] `$impeccable optimize`**: load the in-tab engine as its own wasm on the "In browser" pick; serve and resume
   `/v1/browser/*` by byte range; build the snippet only while Snippets is open, debounced for large states.
4. **[P3] `$impeccable harden`**: Pulling from the server's signal and a fresh runtime import after a failed fetch;
   focus to Run when Stop leaves; relationships only to ids that exist; tooltips out of accessible names and fewer term
   stops; the preset announcement and focus on new rows; the popover's focus-out and Escape; the skip link under the
   drawer.
5. **[P3] `$impeccable optimize`**: transform-based bars, the shortcut glyphs without extra subsets, one lockup symbol;
   release the tab's session when a server row is picked and say its memory cost.
6. **[P3] `$impeccable adapt`**: 44 px hit areas under a coarse pointer, Stop and "Show the command" included.
7. **[P3] `$impeccable polish`**: a term's tooltip clear of the in-flight banner; a second cue for the current sidebar
   row; then a final pass and a fresh `$impeccable audit`.

## Round history

- Round 1 (brand redesign, 2026-10-01): P0 0 · P1 5 · P2 7 · P3 8, 12/20. The P1s are the undeclared Geist Mono, Run
  and the fact tooltips cut at 320 px, focus hidden under the sticky bar on reverse Tab, borderless empty fields at
  1.10:1 and the 422 location code at 4.47:1.
- Round 2 (fixes, 2026-10-01): P0 0 · P1 0 · P2 5 · P3 9, 17/20. Round 1 found five P1s; each is closed and
  re-measured. Geist Mono: `fonts.css` declares the six faces; platform fonts now report Geist Mono (web font) for the
  select, every readout, the model name, inline code, the wire panes and the snippet, `document.fonts` holds both
  families, no unused-preload warning remains, and both faces load before first paint. 320 px: the bar is one line
  from 310 px with Run whole at 240–304 px (it was cut by 32 px) and wraps rather than cuts below that; the
  right-column fact tooltips open leftwards, 0 px cut at 320, 360 and 390 (were 38–45 px). 2.4.11:
  `scroll-padding-top` on the root; the Shift+Tab passes at 1280 and 390 leave nothing hidden (curl, the type radio
  and Edit land at y=76) and nine probes at 1280, 390 and 320 come out clear. 1.4.11: every box you type in wears the
  `--code-muted` ring, 3.14:1 on its fill and 3.45:1 on paper in light, 4.18:1 and 4.61:1 in dark, on all 11 fields of
  the builder state, the empty Yes and No means fields included. 1.4.3: `--danger` is `#b42318`, the location code
  5.09:1 (was 4.47:1). Round 1's P2s on text-only enlargement (labels wrap, Run wraps whole), the forced-colours Run
  (GrayText on Canvas, 14.02:1 and 13.98:1, was 1.24:1) and the reduced-motion drawer (0 s, was 200 ms) are closed too;
  its DESIGN.md P2 was not scored (DESIGN.md and its sidecar landed during this round and match the build on
  spot-check). New: the wrapped bar outgrows the 64 px its scroll padding and popover use (P2), the P max tooltip at
  961–1099 px (P2), the current row's lightness-only cue (P3).
- Round 3 (regression check of the final build, 2026-10-01): P0 0 · P1 0 · P2 5 · P3 9, 17/20. Every round-1 and
  round-2 fix holds: Geist Mono renders, no text pair under AA in either scheme (the segmented legend at 13px/400
  5.74:1 and 7.66:1, the Onest state placeholder 5.22:1 and 6.94:1, Run 19.8:1 at rest and busy, held 2.34:1 and
  3.14:1 as an inactive control), the Tab passes at 1280 and 390 hide nothing, 320 px, 200% zoom and 200% text lose
  nothing, reduced motion and forced colours hold on Run's new face-and-sizer structure, whose sizer is unfocusable,
  silent and still, and `detect` is clean on all 16 live scans and the file scans. Of round 2's P2s, the P max
  tooltip at 961–1099 px is closed (0 px cut at every width from 961 to 1180); the wrapped-bar finding stays open
  (the 70 px bar and 82 px padding leave 3–7 px of a wrapped-bar control visible, but a 17 px fact term at 300 px is
  still fully hidden and the phone popover still covers half of Share and Run) and now also covers every run at
  390–400 and 320–340 px, where Run grows from 64 to 106 px and wraps the bar (a 34 px jump, 0.047 shift); the
  repeated-status, phone-run-focus and 400 KB P2s stay open. New: the leftward fact tooltips cut 64–84 px at
  721–960 px (P2).
- Round 4 (verification of round 3's P2s, 2026-10-01): P0 0 · P1 0 · P2 4 · P3 9, 17/20. The Confidence-tooltip P2 is
  closed: no tooltip is cut at any of 33 widths from 320 to 1920 px in either preset (721, 740, 760, 768, 800, 850,
  900, 940 and 960 px included). The top-bar P2 is closed for the default configuration: the bar holds one 70 px line
  at rest at every 5 px from 280 to 440 and during a run from 310 to 440 (it wraps only mid-run at 280–305 px); at
  390 px a run no longer moves it (bar 70 → 70 → 70 px; the 0.025 shift left on the answer's arrival is the content's,
  as at 412 and 430 px); at 300 px at rest a 17 px fact term now lands clear. It stays open, narrowed, under 200% text:
  the bar wraps to 113 px at 390–415 and 280–295 px, where Shift+Tab leaves the 28 px type segment fully hidden at
  y=83 and the phone popover overlaps Run by 17 px. Share's new phone glyph (389 px and below) keeps the name "Share
  link", reads 5.74:1 and 7.66:1, takes the 19.8:1 ring, has no tooltip, and opens the popover on a tap. Nothing else
  moved: 1,518 text measurements without a failure (min 5.09:1 light, 5.62:1 dark, rings 3.14:1), Tab passes with no
  hidden stop, and `detect` clean on 16 live scans and the file scans. The repeated-status, phone-run-focus and 400 KB
  P2s were not re-measured (the code they live in is unchanged since round 3).
- Round 5 (fresh-context audit of the fix pass, commit ef50236, 2026-10-03; the "In browser" rows, the in-tab run, the
  CLI handoff and the public server audited for the first time): P0 0 · P1 1 · P2 6 · P3 11, 16/20. The round-3 critique
  (28/40, P1 ×2), re-verified on real models: its P1 on the unguarded, unstoppable 467.7 MB first run is closed (the
  size line before Run in the main column at every width on both servers, autorun waiting, Stop by keyboard and touch,
  Stop while pulling, another pick stopping the download; `round5/probe-static.json`, `round5/tab-desktop.json`,
  `round5/tp-public-phone.json`, `round5/tab-drop.json`); its P1 on silent progress (4.1.3) is closed in Chrome's
  accessibility tree (each stage and every tenth said once, Run's description the banner's words, the bar hidden), not
  checked with a screen reader; its stage-line P2 is closed (no Pulling under the 400 ms grace: four real pulls took
  208–259 ms, `round5/real-pull.json`; one Downloading stage across the files; Starting once; the banner sticky), with a
  residual on networks slower than the grace (P3 17); its in-tab failure P2 is closed (the page's words, one next step,
  recovery without a reload), with a residual: a failed runtime fetch leaves later runs on WASM (P3 17); the held Run's
  faded tooltip is closed (none on hover or focus); its run-command framing P2 stays open by plan decision Q2 (the
  public server still says "This server has not pulled decider-4b", and the local one still leads with the install
  line). Its minor notes are unchanged: two sizes for one model on one screen (467.7 MB in the tab, 811.8 MB for the
  CLI), "Received · HTTP 200" and decider-0.8b-v1 for a tab run (the API's own bytes, by DESIGN.md). Round 4's 13
  findings all reproduce on this build (`round5/carried.json`); two gain cases from the new states (the banner's words
  and Snippets' summary hidden under the wrapped bar with 200% text; Stop and "Show the command" at 24 px). New: focus
  hidden under the phone banner at the default text size (P1, closed in round 6), the weights restarting after any
  interruption (P2), the first paint waiting for a 912 KB wasm (P2), the in-tab run's residual edges, Stop's focus loss
  and the 583 MB kept session (P3). No contrast failure in 2,360 measurements; `detect` clean on 43 scans.
- Round 6 (fresh-context re-verification of round 5's P1 after its fix, 2026-10-03; the working tree on ef50236 with
  `follow_heights`, `show_text_field` and the measured offsets): P0 0 · P1 0 · P2 8 · P3 12, 16/20. Round 5's P1 is
  closed. Round 5's walk (a server run's answers on the page while decider-0.8b downloads into the tab; round 5's
  16-stop Shift+Tab and Tab walk, whole cycles, and cycles with a builder open) at 320x640, 360x740, 390x844, 430x932
  and 1280x800, 100% and 200% text, light and dark, 2,832 stops: 2,172 page stops whole or filling the room left, none
  hidden, no ring cut, all 480 fact-term and 240 editor stops whole; the scroll padding in flight is bar + banner + 12
  px (172 px on phones, round 5: 138; 122 at 1280; 317–373 with 200% text), and Shift+Tab puts Certainty and P max at
  y=172–189 at 390 px under a banner ending at 160 (round 5: 142–159, hidden) (`round6/p1-walk.json`,
  `round6/screens/p1-390x844-text100-light-shift-term.png`, `round6/screens/p1-430x932-text100-light-shift-term.png`).
  Round 5's `focus-terms.mjs`, `focus-hidden.mjs` and `banner-geometry.mjs`, re-run unchanged, find no term hidden at
  320–720 px (round 5: Certainty and P max at 390, Confidence and Answer at 430), 0 px hidden in the 200%-text walks
  (round 5: Snippets' summary and the questions editor wholly hidden) and the banner 0 px under the bar at all 13 sizes
  (round 5: 34–43 px) (`round6/focus-terms.json`, `round6/focus-hidden.json`, `round6/banner-geometry.json`). Landscape
  (844x390, 932x430) and 200% and 400% zoom (640x400, 320x256) hide no stop at default text. The regression check of
  what the fix touched passes: through Pulling and Starting held by a proxy, Downloading held by throttling, and Running
  and the answers recorded as they passed, at 300–1280 px with both text sizes, the properties equal the rendered boxes
  (0 mismatches in 13 runs), the banner sticks at the bar's bottom, also as the bar wraps mid-run (300 px, 70 → 104 px)
  and as the download's words take a second line (430 px, 69 → 90 px), Stop stays hit-testable, and the root's style
  changed 3–6 times a run, never at rest, without a window error (`round6/stages.json`); 12 viewport and text-size
  changes mid-flight were followed, with no write in 5 idle seconds (`round6/regress-jumps.json`); the bar matches its
  property at 34 width and text pairs, and whole cycles at rest with 200% text at 280–430 and 1280 px hide nothing
  (`round6/regress-bar.json`); the phone popover hangs 8 px under the bar in 18 width, text and flight combinations,
  over Run by 0 px (round 5: 17 px); the Raw exchange and Snippets jumps and "Show the command" land 11.5–12.5 px under
  the bars and the skip link and "State and questions" bring the page to its top, also with reduced motion; a click into
  an editor scrolls 0 px and Tab brings it whole; the public opening page and an in-tab run on it hide nothing
  (`round6/public.json`); `detect` reports 0 findings on 20 live scans and the file scans. Finding 2 is narrowed (the
  bar's wrap no longer hides focus, Run or the banner) to the segmented rows, now measured pushing the snippet's copy
  key off a phone's screen. Round 5's other findings reproduce (`round6/carried.json`) or stand on unchanged code. New:
  the in-flight bars crowd small or zoomed screens (P2), the skip link's fragment and Back (P2), a term's tooltip over
  the in-flight banner (P3).
- Supersedes the three Notion-world rounds of 2026-09-28 (closed at P0 0 · P1 0 · P2 4 · P3 8, 17/20): its repeated-
  status, phone-run-focus and 400 KB P2s carried over into round 1, its field-boundary P2 became round 1's P1 4, and
  its literal-shadow and collapsed-workspace-name P3s went away with the old look.
