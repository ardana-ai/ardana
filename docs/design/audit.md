# Playground audit

Impeccable `audit` of `crates/ardana-playground` in its ardana.ai brand redesign (Onest and Geist Mono, ink on paper,
the ink Run pill, hairline rows, a scheme-following inverse) and of `crates/ardana-server/placeholder/`, run by a
fresh-context agent against a live `ardana serve` (decider-2b; for round 4 the build of 17:33, its served wasm and
stylesheets byte-identical to `dist/` and `styles/`) on the installed Chrome 154 through Playwright: 1280x800,
390x844, 320 wide (the top bar every 5 px from 280 to 440 at rest, while a run is in flight and with 200% text, every
tooltip at 33 widths from 320 to 1920 px, the bar rows at eight widths across the container query), 200% page zoom,
200% text-only resize, WCAG 1.4.12 text spacing, reduced motion, forced colours (light and dark palettes), a touch
pointer, `prefers-color-scheme: dark` and a scheme switch at runtime; the empty, loaded, results, 422, stale,
builder-open (choice, noul, a new question, score levels up to 10), score preset, invalid-JSON, Share popover, copy
(granted and refused), tooltip, busy, drawer and collapsed states; composited contrast of every text node,
placeholder, field and its ring, icon, segment ring, bar, mark and tooltip in both schemes (1,518 text, 54
placeholder, 484 component and 146 tooltip measurements over 24 state and scheme pairs); Run's face-and-sizer
structure and Share's phone glyph (width, visibility, focusability, accessible name, focus ring, contrast, touch); a
keyboard-only pass (the whole Tab and Shift+Tab order at both viewports, probes of controls and fact terms lying under
the sticky bar at 1280, 390, 360, 320 and 300 px and with 200% text, skip link, presets and Restore previous from the
sidebar and the phone drawer, the type switch by arrow keys, Edit, Add and Remove at both bounds, Remove question,
Ctrl/Cmd+Enter, Ctrl/Cmd+\, Escape, Share and both copy keys, Run while running); live-region text changes and DOM
mutations; the CDP accessibility tree; the faces Chrome draws (CDP platform fonts), preloads, FOUT and layout shift,
at load and during a run; network sizes, paint timings and keystroke latency at 4, 40 and 400 KB of state;
`impeccable detect --json` on `index.html`, `styles/`, `placeholder/` and the four live states at both viewports in
the light scheme and, through the installed Chrome with its native theme forced dark, in the dark one. Evidence
(scripts, JSON, notes, screenshots): round 1 in `tmp/evals/audit/`, rounds 2 to 4 in `tmp/evals/audit/round2/` to
`round4/`, each indexed by its `notes.txt`. Not exercised: a real screen reader (live regions were observed as DOM
mutations and in the accessibility tree), synthesized touch gestures (the page has no drag surfaces; a tap was
simulated for tooltips and Share), Safari, Firefox, Windows and Linux font fallback, and the Pulling state of a
first-run download (the server runs offline).

P0: 0 · P1: 0
P2: 4 · P3: 9

## Audit Health Score

| # | Dimension | Score | Key finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | 3 | AA met in the default configuration; with 200% text the top bar still wraps at 390–415 px and a 28 px control can sit hidden under it; a repeated 422 is not re-announced |
| 2 | Performance | 3 | 451 KB on the wire (wasm 316 KB Brotli), both faces loaded before first paint, CLS 0.017–0.035 at load; a run no longer moves the bar; 111–121 ms per keystroke at 400 KB |
| 3 | Responsive design | 3 | One 70 px bar at rest from 280 px and during a run from 310 px; no tooltip cut at any of 33 widths; 200% text still wraps the bar at 390–415 px; touch targets 28 px |
| 4 | Theming (token discipline) | 4 | Every colour a `:root` token with a dark twin, every text pair ≥ 5.09:1 light and ≥ 5.62:1 dark, Run 19.8:1 in both schemes, forced colours legible; only the current sidebar row's cue is weak in dark (2.21:1) |
| 5 | Implementation integrity | 4 | One coherent ardana.ai world; `detect` reports 0 findings on all 16 live scans and on every file scan; DESIGN.md describes the build |
| | Total | 17/20 | Good |

Implementation integrity verdict: pass. The page is ardana.ai at work and nothing else: `base.css` carries the
landing's `src/app.css` tokens by name and value and derives its own from them, the groundhog lockup comes from the
logo kit (`img "Ardana"`, `currentColor`, pure white on dark), Run is the one ink pill and keeps the landing's
Copy-pill width above 720 px, questions are hairline catalog rows, each answer is the one ink line among gray ones,
the command boxes, copy keys and the curl `$` prompt are the landing's, red appears only on faults, and no literal
colour exists outside `:root`. CDP reports "Geist Mono (web font)" for every mono run and "Onest (web font)" for every
word, in the playground and in the placeholder. `impeccable detect` reports 0 findings on `index.html`, `styles/`,
`placeholder/` and all 16 live scans (four states, two viewports, light and dark); the pinned Geist Mono is an
`overused-font` exception in `.impeccable/config.json`, with its reason. DESIGN.md and `.impeccable/design.json`
describe the build on every point spot-checked in round 3.

## Executive summary

- Audit Health Score 17/20 (Good): no P0 or P1. Round 4 closed round 3's Confidence-tooltip P2 and the run-time half
  of its top-bar P2; what is left of the bar finding needs 200% text.
- 13 findings: P0 0, P1 0, P2 4, P3 9.
- Top issues: with enlarged text the top bar still wraps at 390–415 px (focus can hide under it, the phone popover
  overlaps Run); a repeated 422 is not re-announced; a phone run leaves focus on Run; typing in a 400 KB state lags.
- Next: `$impeccable adapt` for the bar under enlarged text, `$impeccable harden` for the announcements and focus,
  `$impeccable optimize` for the snippet, then `$impeccable polish`.

## Findings

### P1

None open. Round 1's five stay closed in this build; see Round history.

### P2

1. **With enlarged text the top bar still wraps, and what assumes one line breaks** (round-3 P2, narrowed)
   - Location: `styles/shell.css:190-202` (`flex-wrap: wrap` at 195), `styles/base.css:57` and `:103`
     (`--topbar: 70px`, `scroll-padding-top: calc(var(--topbar) + var(--space-3))`), `styles/shell.css:387-393`
     (below 720 px the popover is fixed at `calc(var(--topbar) + 8px)`).
   - Category: Accessibility (Responsive).
   - Evidence: At 100% text the bar is one 70 px line at rest at every 5 px from 280 to 440 and while a run is in
     flight from 310 to 440; it wraps only mid-run at 280–305 px (`round4/topbar-sweep.json`). With 200% text it is one
     73 px line at 300–385 and 420–440 px but wraps to 113 px at 390–415 and 280–295 px, while the scroll padding is
     82 px: Shift+Tab onto the checked type segment (28 px) at y=83 left it fully hidden at 390 and 400 px, and the
     phone popover (top 78 px) overlaps Run on the bar's second row by 17 px (`round4/obscured-r4.json`,
     `screens/popover-390-text200.png`). At 360 px with 200% text, and everywhere at 100% text from 310 px, every probe
     is clear.
   - Impact: People who enlarge text on the most common phone widths (390–415 px) can lose sight of focus moving
     backwards, and of part of Run while the popover is open.
   - WCAG: 2.4.11 Focus Not Obscured (Minimum) (AA) under text-size settings; met at the default.
   - Recommendation: Drive `scroll-padding-top` and the phone popover's `top` from the bar's measured height
     (`ResizeObserver` writing a root custom property), or give the 390–415 px band the 389 px tier's compact bar when
     text is enlarged (an em-based breakpoint follows the user's text size).
   - Suggested command: `$impeccable adapt`

2. **A repeated outcome is not re-announced** (carried over)
   - Location: `src/ui/mod.rs:196-198` (`run-status`), `src/ui/mod.rs:262-281` (`run_status`).
   - Evidence: The same invalid request run twice produced 0 mutations in `run-status` the second time
     (`round3/live.json` second422; the code is unchanged since).
   - Impact: Screen-reader users hear nothing after pressing Run again on the same error.
   - WCAG: 4.1.3 Status Messages (AA), in spirit: the message exists but is not sent again.
   - Recommendation: Empty the region for one frame before refilling it; the e2e suite reads the final text, which does
     not change.
   - Suggested command: `$impeccable harden`

3. **On a phone a run scrolls to the answers while focus stays on Run** (carried over)
   - Location: `src/ui/mod.rs:187-192`, `src/ui/mod.rs:295-308` (`scroll_to_result`).
   - Evidence: At 390 a keyboard run scrolled the first question to y=82 with focus on Run; the next Tab went to State
     and scrolled back to the top (`round3/misc.json` phoneRun).
   - Impact: Keyboard and switch users on a phone lose the answers they were just shown.
   - Standard: WCAG 2.4.3 Focus Order (as practice).
   - Recommendation: Move focus to the fault or the first question when the columns stack; `answers_match_api`, which
     asserts Run keeps focus, would have to accept it.
   - Suggested command: `$impeccable harden`

4. **Typing in a 400 KB state costs 111–121 ms per key** (carried over)
   - Location: `src/ui/snippets.rs:15` (the snippet memo over `deck.request()`), `src/ui/snippets.rs:37-39`
     (its `<pre>`).
   - Evidence: A long task of 111–121 ms on every keystroke with Snippets open (event durations up to 192 ms), none
     with it closed (`round3/perf.json`, `round3/perf2.json`). At 4 and 40 KB keystrokes stay frame-bound
     (processing ≤ 18 ms).
   - Impact: Long transcripts type sluggishly; the workaround is to collapse Snippets.
   - Standard: Interaction to Next Paint (good ≤ 200 ms; here 120 ms at the median, up to 192 ms).
   - Recommendation: Build and render the snippet only while Snippets is open, debounced (about 150 ms) above a size
     threshold.
   - Suggested command: `$impeccable optimize`

### P3

5. **Bars and the shell animate layout properties** (carried over). Location: `styles/page.css:309,318-322`
   (`bar-grow` animates `width`, 600 ms), `styles/shell.css:9` (`grid-template-columns`, 200 ms). Category:
   Performance. Impact: layout work for every bar of every run and on every collapse. Standard: compositor-only
   animation. Recommendation: grow the fill with `transform: scaleX()` from the left. Suggested command:
   `$impeccable optimize`.
6. **`aria-controls` names ids that exist only while open** (carried over). Location: `src/ui/topbar.rs:158`
   (`share-panel`), `src/ui/questions.rs:321` (`{dom}-program`). Category: Accessibility. Impact: three dangling
   relationships while closed (`round3/misc.json`). Standard: ARIA 1.2 (`aria-controls` must reference an element).
   Recommendation: render the panels hidden, or set `aria-controls` only while expanded. Suggested command:
   `$impeccable harden`.
7. **Fact terms and tooltips over-announce** (carried over). Location: `src/ui/questions.rs:423`,
   `styles/controls.css:316` (`content: attr(data-tip)`). Category: Accessibility. Impact: every fact name is a
   `dfn tabindex="0"` (four extra stops per answered choice question), and a shown CSS tooltip joins the accessible
   name: keyboard focus names Run "Run Send the state and questions Ctrl+Enter or ⌘↵" (it still starts with "Run",
   and the unseen sizer adds nothing) and a fact term reads its meaning twice (`round3/tipname.json`). Standard:
   accname (generated content counts as content). Recommendation: `content: attr(data-tip) / ""`, as the curl prompt
   already does; drop the terms' tab stops or keep one per question. Suggested command: `$impeccable harden`.
8. **Presets and added rows leave screen-reader and keyboard users behind** (carried over). Location:
   `src/ui/sidebar.rs:100-112`, `src/ui/builder.rs:190-215,360-367`. Category: Accessibility. Impact: loading a preset
   announces nothing (`round3/live.json` preset); Add option and Add level keep focus on the Add button, which the new
   row pushes below the fold at 1280x800 (`round3/keyboard-log.txt`). Standard: WCAG 4.1.3 and 2.4.7 as practice.
   Recommendation: announce "Loaded <preset>" in the notice region; focus the new row's first field. Suggested
   command: `$impeccable harden`.
9. **The Share popover outlives its focus** (carried over). Location: `src/ui/topbar.rs:119-151`,
   `styles/controls.css:322` and `styles/shell.css:311` (both `z-index: 30`). Category: Accessibility. Impact: Tab out
   of the popover leaves it open and Escape no longer closes it once focus is outside (`round3/keyboard-log.txt`);
   Run's tooltip draws over the popover's top-right corner, over empty space only. Standard: disclosure pattern
   practice. Recommendation: close on `focusout` beyond `#share` and handle Escape at the window while open. Suggested
   command: `$impeccable harden`.
10. **The skip link stays reachable under the open drawer** (carried over). Location: `src/ui/mod.rs:195,205` (only
    `.main` is `inert`), `styles/shell.css:17-34`. Category: Accessibility. Impact: Tab from the drawer's last row
    reaches "Skip to the page"; activating it targets the inert page and drops focus to `<body>` with the drawer still
    open (`round3/misc.json` skipOverDrawer). Standard: WCAG 2.4.3 as practice. Recommendation: make the skip link
    inert while the drawer is open, or let it close the drawer first. Suggested command: `$impeccable harden`.
11. **Two glyphs and a doubled logo add weight** (carried over). Location: `src/ui/sidebar.rs:151` and
    `src/ui/topbar.rs:41,67` (⌘ and ↵), `styles/fonts.css:25-63` (Onest's math and symbols subsets),
    `src/ui/logo.rs:8,13`, `src/ui/sidebar.rs:78`, `src/ui/topbar.rs:46`. Category: Performance. Impact: ⌘ and ↵ still
    pull two Onest subsets (44.1 KB on the wire) for two characters; the 71.6 KB traced lockup adds about 12 KB of
    Brotli to the wasm and is parsed into the DOM twice. Standard: asset budget. Recommendation: draw the shortcut
    glyphs as SVG (or spell them out), and render the lockup once as a `<symbol>` referenced by `<use>`. Suggested
    command: `$impeccable optimize`.
12. **Touch targets are 28–40 px** (carried over). Location: `styles/controls.css:113-125` (`.button-sm`, small icon
    buttons at 28 px), `controls.css:145` (segments 28 px tall). Category: Responsive. Impact: Remove option, Remove
    level, the type segments, Edit and Add option are 28 px tall on a phone (Share's glyph is 32 px). Standard: WCAG
    2.5.8 (AA) passes at its 24 px minimum; 2.5.5 (AAA) asks 44 px. Recommendation: 44 px hit areas under
    `(pointer: coarse)`, visual size unchanged. Suggested command: `$impeccable adapt`.
13. **The current "On this page" row is told apart by lightness alone** (carried over). Location:
    `styles/shell.css:139-141`. Category: Accessibility (Theming). Impact: the current row differs from the others only
    by ink against pencil gray: 3.45:1 in light, but #ededed against #a1a1a1 is 2.21:1 in dark, and forced colours draw
    every row alike. Standard: WCAG 1.4.1 practice (a lightness difference of 3:1 or more stands in for a non-colour
    cue). Recommendation: a second cue that survives both schemes and forced colours, such as weight 500 or a 2 px ink
    rule at the row's start. Suggested command: `$impeccable polish`.

## Patterns and systemic issues

- The top bar's height is a constant in a box that can still grow: `--topbar` (70 px) feeds the scroll padding and the
  phone popover's offset, and with enlarged text the bar wraps to 113 px at 390–415 px (finding 1). Measure the box, or
  give enlarged text the compact bar.
- Status and focus after an action are still handled case by case: a repeated status, a preset, a new row and a phone
  run each leave the user to find out what happened (findings 2, 3, 8).

## Positive findings

- The top bar now holds one line where it matters: 70 px at rest at every width from 280 px and while a run is in
  flight from 310 px; at 390 px a run no longer moves it (the 0.025 shift left when the answer arrives is the answer's
  own, as at 412 and 430 px, where the bar never wrapped).
- Share's phone face is sound: at 389 px and below a 32 px link glyph with its word visually hidden, the name still
  "Share link", the glyph 5.74:1 in light and 7.66:1 in dark, the 2 px ring 19.8:1, no tooltip, and a tap opens the
  popover with no tooltip left behind.
- Fact tooltips choose their side per layout: none is cut at any of 33 widths from 320 to 1920 px in either preset.
- Run's structure is sound: above 720 px the pill is 106 px at rest, held and busy; the "Running" sizer is hidden,
  `aria-hidden`, unfocusable and still; Run's name is "Run" at rest (with its held reason as description) and
  "Running" while busy; reduced motion holds the caret steady; forced colours draw Run in Highlight at rest and
  GrayText on Canvas when held or busy.
- Both brand faces render and load well: Onest and Geist Mono self-hosted, same origin, the two latin files preloaded
  and used, loaded before first paint (no FOUT); the placeholder draws them too.
- Contrast: no text pair under AA in any of 24 state and scheme pairs: 5.09:1 or more in light, 5.62:1 or more in
  dark; the legend 5.74:1 and 7.66:1; placeholders 5.22:1 and 6.94:1; Run's label 19.8:1 in both schemes; the type
  ring 3.14:1 on its fill and 3.45:1 on paper in light, 4.18:1 and 4.61:1 in dark; the focus ring 18–19.8:1 in light
  and 17.9–19.8:1 in dark. The curl `$` prompt (3.14:1 and 4.18:1) is decoration with empty alt text.
- Reflow: 320 px, 200% zoom, 200% text and 1.4.12 spacing lose nothing; the sidebar's labels wrap; bar rows restack
  whenever the questions column is under 30rem, with no overlap.
- Focus: the Tab and Shift+Tab passes at 1280 and 390 leave nothing hidden; the 2 px ring shows on every stop.
- Reduced motion leaves nothing moving at 1280 and 360 px; forced colours border Share's glyph in the system text
  colour and keep Run in Highlight; a tap on a fact term leaves no tooltip (`round4/quick-media.json`). Keyboard
  behaviour and the live regions live in code unchanged since round 3, where they were verified (`round3/`).
- `impeccable detect`: 0 findings on `index.html`, `styles/`, `placeholder/` and 16 live scans, light and dark.

## Recommended actions

1. **[P2] `$impeccable adapt`**: under enlarged text keep the bar to one line at 390–415 px, or drive the scroll
   padding and the phone popover's offset from the bar's measured height; re-test 280–440 px at 200% text.
2. **[P2] `$impeccable harden`**: re-announce a repeated status; focus the answers after a phone run.
3. **[P2] `$impeccable optimize`**: build the snippet only while Snippets is open, debounced for large states.
4. **[P3] `$impeccable optimize`**: transform-based bars, the shortcut glyphs without extra subsets, one lockup symbol.
5. **[P3] `$impeccable harden`**: tooltips out of accessible names, fewer term tab stops, `aria-controls` while open
   only, the preset announcement and focus on new rows, the popover's focus-out and Escape, the skip link under the
   drawer.
6. **[P3] `$impeccable adapt`**: 44 px hit areas under a coarse pointer.
7. **[P3] `$impeccable polish`**: a second cue for the current sidebar row; then a final pass and a fresh
   `$impeccable audit`.

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
- Supersedes the three Notion-world rounds of 2026-09-28 (closed at P0 0 · P1 0 · P2 4 · P3 8, 17/20): its repeated-
  status, phone-run-focus and 400 KB P2s carried over into round 1, its field-boundary P2 became round 1's P1 4, and
  its literal-shadow and collapsed-workspace-name P3s went away with the old look.
