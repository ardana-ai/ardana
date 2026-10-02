---
target: the Ardana playground
total_score: 28
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 2
target_identity: "file:/Users/tigran/projects/ardana-ai/ardana/crates/ardana-playground"
timestamp: 2026-10-01T23-02-41Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (ardana.ai brand world, round 1)

Method: dual-agent (A: fresh-context design review · B: fresh-context detector and browser evidence), run in parallel and isolated; the parent saw neither until both returned. Target `crates/ardana-playground`, live at `ardana serve` on 127.0.0.1:8712 with decider-2b, compared with the landing page's own build (ardana.ai `/`, `/models/`, `/models/decider-2b/`). Mode: Operate. Evidence: Playwright on the installed Chrome at 1280x800 and 390x844, light and dark, with touch emulation for one check (`tmp/evals/critique-a/`); `impeccable detect --json` on `index.html` and on the empty, loaded, results and 422 URLs at both viewports, in the machine's dark scheme and again forced light, plus the in-page overlay (`tmp/evals/critique-b/`).

## Design Health Score
| # | Heuristic | Score | Key issue |
|---|---|---|---|
| 1 | Visibility of system status | 3 | Busy pill with the caret, latency and tokens, per-question stale tags; the reason Run is held sits in a banner that scrolls away |
| 2 | Match between system and real world | 3 | Fact tooltips state formulas; "noul" and the score "fit" go unexplained once the empty-state primer is gone |
| 3 | User control and freedom | 3 | Escape closes everything with focus restored; Restore previous is one level deep and lives only in the sidebar |
| 4 | Consistency and standards | 3 | One system end to end; terminal gray means both "type here" and "copy this"; the gray bar fill means losing option, noul and stale alike |
| 5 | Error prevention | 3 | Live JSON validation holds Run; a library model's 2.7 GB pull starts on Run with no warning |
| 6 | Recognition rather than recall | 3 | Presets and shortcuts visible; JSON errors cite line and column in an editor with no line numbers |
| 7 | Flexibility and efficiency of use | 3 | Ctrl/⌘+Enter everywhere, share links, three snippet languages; the wire panes have no copy key |
| 8 | Aesthetic and minimalist design | 2 | Quiet and monochrome, but the answer is smaller than the title, the state box is the heaviest object, terminal gray covers 41–42% of the column |
| 9 | Help users recognize, diagnose, recover from errors | 3 | The 422 names `body › questions › department` and repeats it on the question; no next step offered; the location chip reads 4.47:1 |
| 10 | Help and documentation | 2 | Formula tooltips and the primer help; the primer disappears after the first question, nothing explains calibration or "fit" |
| **Total** | | **28/40** | **Good (bottom of the band)** |

## Design Specificity Verdict
**LLM assessment:** the materials are authored for Ardana; the composition is the category's model playground. The groundhog lockup at the landing's exact size (149.78x26), Onest at the landing's weights and tracking, the landing's "default" pill as the tag, the command box with its 40px copy key and 1.6s check, hairline catalog rows, the 13px-over-15px fact row, and the landing's one ornament (the blinking block caret) reused as the in-flight signal inside Run all read as ardana.ai. The sidebar, sticky bar, editor-left/results-right and raw exchange could belong to any LLM console; the decision itself (the one thing this product owns) is drawn at table-cell size with every option in the same ink, so the landing's "hand the eye to one line" never crosses over.

**Deterministic scan:** `impeccable detect` exit 0 with no findings on `index.html` (its body is empty; the markup is rendered from Rust) and on all eight URL scans; a canary injected after render proved the scans saw the rendered states. Those scans ran in the machine's dark scheme; forced light, the 422 state gives one finding at both viewports, `low-contrast` 4.47:1 on `code.fault-loc` (`#c4261d` on `#f9dcd9`, `styles/page.css` `.callout .fault-loc`, tokens in `base.css`), confirmed by A's own measurement. Chrome also warns that the preloaded `geist-mono-latin-wght-normal.woff2` is never used: `styles/fonts.css` declares only Onest, and CDP platform fonts show every mono string drawn in Menlo. The URL scans cannot apply DESIGN.md (no `design-system-*` rules over http), and DESIGN.md still codifies the retired Notion system.

**Visual overlays:** injection succeeded in an automation Chrome (results and 422, both viewports): the results state reported no anti-patterns; the 422 state reported the same `low-contrast` finding. Two overlay hits (`text-occlusion`, `dark-glow`) were the overlay's own label and highlight, false positives. No persistent `[Human]` tab remains; screenshots are in `tmp/evals/critique-b/overlay/`.

## Overall Impression
It looks and moves like ardana.ai, down to the caret, until a mono string appears: the terminal voice is Menlo, not Geist Mono, so half the brand's type never arrives. The biggest opportunity is the brand's own move: make the answer the one ink line and quiet everything around it.

## What's Working
1. Token-exact transfer: `base.css` reuses `app.css` names and values; title, lede, titles, tag, fact row, rows, copy key and logo measure identical to the landing; a hue scan finds nothing but the agreed fault red; zero external requests.
2. Honest state in the brand's vocabulary: the caret marks a run in flight (steady under reduced motion); stale answers keep their figures and lose their ink; the answer is marked by ink, a check and weight, not colour alone.
3. Keyboard and contrast craft: 29 tab stops in order, each with the landing's 2px ink ring 3px out in both schemes; focus returns correctly from Share, the drawer and presets; zero AA failures among 81 text elements apart from the fault chip; no overflow at 390 with long ids, many options and emoji.

## Priority Issues
- **[P1] Geist Mono never loads; every mono string is Menlo.** Why: one of the three decisions the user confirmed; the terminal voice carries every figure, model name, state and snippet, and off macOS it drops further. Fix: add the six `@font-face` rules of `@fontsource-variable/geist-mono` 5.3.0 to `styles/fonts.css` (the files already ship in `fonts/`), then confirm with CDP platform fonts. Suggested command: $impeccable typeset
- **[P1] The 422 location chip reads 4.47:1 in light.** Why: the part of a refusal that names the failing question is the faintest text on the page (WCAG 1.4.3). Fix: darken `--danger` (about `#b42318`) or lighten the chip's tint. Suggested command: $impeccable harden
- **[P2] The answer is not the one ink line.** Non-answer names and figures are ink too; the answer is 15px/600 under a 41px title; the 416px state box outweighs it. Fix: losing options in the gray voice, the answer's figure at 600, a noul answer in ink. Suggested command: $impeccable layout
- **[P2] Terminal gray as a panel and form fill.** 41–42% of the main column against 1.5–6% on the landing; editable fields and empty wire panes share the copy-me material. Fix: plain muted text for empty panes; give editable boxes an edge of their own so "type here" and "copy this" differ. Suggested command: $impeccable quieter
- **[P2] Answer encodings collide.** The noul answer, a losing option and a stale answer share one gray; a score level's fit figure sits closer to the next level than its own. Fix: noul in ink unless stale; tighten the fit line to its level. Suggested command: $impeccable polish

## Persona Red Flags
**Alex (power user, the audience):** the JSON error "line 5, column 19" points into an editor with no line numbers; the Sent and Received panes look like command boxes but have no copy key; Run grows from 64 to about 112px while running, moving Share under the pointer.

**Sam (accessibility):** the 4.47:1 fault chip; the reason Run is held scrolls away for sighted keyboard users; on a touch phone the Run tooltip stays up after the tap and covers the first question.

**Riley (stress tester):** broken JSON leaves Snippets showing the last valid request; a library model's pull starts with no warning or cancel; renaming an option after a run shows the stale message up to four times. Long ids, many options, emoji and a 2,000-character state all wrap cleanly.

## Minor Observations
- The disclosure toggles rotate the landing's navigation arrow to point down, which reads as "download"; the landing uses the arrow for navigation only.
- Dark mode draws the logo and the Run pill `#ededed`; the kit says white on `#0a0a0a`.
- `--bar #858585` and `--line-strong #d4d4d4` are grays the landing does not have.
- The current sidebar row turns 500; the landing's current nav link changes only to ink.
- After copying the share link, the copy key's tooltip covers the note under it.
- "Playground" in the crumb and "Ardana playground" in the title name the page twice; "Restore previous" does not say what it restores.

## Questions to Consider
1. On ardana.ai the thing to copy is the largest type on the screen; why is the decision smaller here than the page title?
2. If terminal gray means "paste this into a terminal", which gray surfaces here earn it, and how should an editable box differ from a copyable one?
3. A noul answer is a single number; why is it drawn in the colour of a losing option?
