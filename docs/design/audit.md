# Playground audit

Impeccable `audit` of `crates/ardana-playground`, run by a fresh-context agent after the last UI fix of W7, against a
live `ardana serve` (decider-2b, qwen3.5-0.8b) on installed Chrome through Playwright: 1280x800, 390x844, 320 wide,
200% page zoom, 200% text-only resize, reduced motion, forced colours, a keyboard-only run (preset, builder edit with a
type switch and back, rename, removals, Run, copy), status-region mutations, the stale-results state, 422 and 413.

P0: 0 · P1: 0
P2: 6 · P3: 9

## Audit Health Score

| # | Dimension | Score | Key finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | 3 | Add option and Add level drop focus to `<body>` when they disable at the upper bound |
| 2 | Performance | 3 | ~17 ms per keystroke at 42 KB of state, ~110 ms at 420 KB (three state parses per keystroke) |
| 3 | Responsive design | 3 | Reflow passes at 320 px and 200% zoom; 200% text-only resize overlaps the rail at 1280 |
| 4 | Theming (token discipline) | 3 | Palette tokenised; ~47 literal material shades repeat across component CSS |
| 5 | Implementation integrity | 3 | Coherent, product-specific; one duplicated rule block, minor token drift |
| | Total | 15/20 | Good |

Implementation integrity verdict: pass. `impeccable detect` finds 0 findings on the live URLs and 0 anti-patterns on
the source (28 design-system drift advisories).

## Round history

- Round 1 (after the W7 build): P0 0 · P1 5 · P2 8 · P3 5, 14/20. The P1s were: the Type toggle destroyed
  criteria on arrow keys; focus dropped to `<body>` after Run, rename and removals; a successful run was never
  announced; the questions-JSON error re-fired `role=alert` on every keystroke; the amber focus ring on paper fields
  was 1.13–1.19:1 against aluminium. All five were fixed, with the critique's P1s (stale results, undo for presets and
  removals) and the adjacent P2s (faults on their channel, phone scroll after a run, live regions from load, lamp and
  field contrast, 200% text resize, the default-model label).
- Round 2 (this report): no P0 or P1 left.

## Open findings

### P2
1. Add option and Add level use `disabled` at the upper bound and drop focus (`src/ui/builder.rs:296`, `:382`); use
   `aria-disabled` with a guard, as RUN does.
2. At ≤720 px a run scrolls the page away from the focused RUN key (`src/ui/mod.rs:55-60`, `:123-150`); move focus
   with the scroll.
3. A repeated status is not re-announced: two 422s in a row mutate `run-status` once; a second copy within 4 s is
   silent (`src/ui/mod.rs:63-65`, `:103-121`, `src/ui/controls.rs:59-73`).
4. Forced colours erase the selected type, the lamps and key outlines; no `forced-colors` block
   (`styles/controls.css:5-13`, `:93-97`, `:105-113`, `styles/base.css:176-191`).
5. 200% text-only resize: the model select covers the Input tokens counter; px breakpoints (`styles/rail.css:5`,
   `:115-130`, `:318`, `:331`, `styles/deck.css:277-297`).
6. Renaming a question with Tab pulls focus back to the id field (`src/ui/builder.rs:78`).

### P3
7. Add question and Add option leave focus on the Add key (`src/ui/channels.rs:31-38`, `src/ui/builder.rs:297-299`,
   `:383-385`).
8. Touch targets are 32–36 px at phone width; WCAG 2.5.8 passes (`styles/controls.css:9`, `:80`).
9. `aria-label` on generic `<pre>` panes (`src/ui/exchange.rs:40`, `:54`, `src/ui/snippets.rs:30`).
10. Duplicated rule block `styles/deck.css:739-767` of `:248-275`; orphaned comment `styles/rail.css:192`.
11. Hard-coded chrome, bevel and well shades, off-scale radii, no `--radius-window`, unused `--glass-edge` and
    `--amber-ghost` (`styles/controls.css`, `styles/rail.css`, `styles/deck.css`, `styles/base.css:18`, `:26`).
12. Three state parses per keystroke (`src/deck.rs:84-91`, `src/ui/cassette.rs:14-17`, `src/ui/snippets.rs:13`).
13. Ctrl/Cmd+Enter works only in the two editors (`src/ui/mod.rs:93-100`).
14. Loading a preset is not announced (`src/ui/console.rs:37-40`).
15. A 413 about the state is shown only in the Questions column (`src/ui/channels.rs:45`, `src/ui/cassette.rs:49-67`).

## Positive findings

- The full run works by keyboard with no traps; type and language toggles are native radio groups; a type switch and
  back restores criteria; removals move focus to the row that took their place.
- RUN uses `aria-disabled` with `aria-describedby` giving its reason; field errors use `aria-invalid` and
  `aria-describedby` and announce only on a valid/invalid flip.
- All text measured ≥ 5.16:1; amber readouts on glass ~10:1; the ink focus ring on aluminium ≥ 10:1.
- No horizontal scroll at 320 px, 200% zoom or 200% text; reduced motion removes the ladder animation and key
  transitions and makes the scroll instant.
- Errors show the API's own `detail`; a 422 marks the named channel; stale results are marked on the rail, the lamp
  and every channel while the figures stay as returned.
- 283 KB wasm (Brotli), two preloaded fonts, first paint ~110 ms.
