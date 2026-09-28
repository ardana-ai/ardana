---
target: the Ardana playground
total_score: 27
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 2
target_identity: "file:/Users/tigran/projects/ardana/crates/ardana-playground"
timestamp: 2026-09-28T16-48-52Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (round 1)

Provenance: dual-agent run. Assessment A (design review) and Assessment B (detector and browser evidence) ran as two isolated fresh-context sub-agents; A never saw detector output. Target `crates/ardana-playground`, live at a local `ardana serve` with decider-2b and qwen3.5-0.8b. Mode: Operate.

## Design Health Score
| # | Heuristic | Score |
|---|---|---|
| 1 | Visibility of system status | 3 |
| 2 | Match between system and real world | 3 |
| 3 | User control and freedom | 2 |
| 4 | Consistency and standards | 3 |
| 5 | Error prevention | 2 |
| 6 | Recognition rather than recall | 3 |
| 7 | Flexibility and efficiency of use | 3 |
| 8 | Aesthetic and minimalist design | 3 |
| 9 | Help users recognize, diagnose, recover from errors | 3 |
| 10 | Help and documentation | 2 |
| | Total | 27/40 (Acceptable, top of band) |

## Design Specificity Verdict
Authored, not interchangeable: the tape-deck fascia carries the mechanism. Amber marks only values the API returned, idle windows stay dark, the winner gets a lit lamp, RUN is the only travelling key, red is spent only on faults. Costs of the metaphor: "channel" jargon, an always-zero output-tokens counter, an oversized cassette well on desktop.

**Deterministic scan**: `impeccable detect` returned 0 findings on `index.html` and on `/`, the loaded share link, the autorun results and a 422 URL at 1280x800 and 390x844, also with `--no-config`. The in-page overlay reported no anti-patterns on the empty, results and builder-open states at both viewports. Planted controls (gradient text, side-tab, low contrast, a late-rendered page) were all caught, so the clean result is not a detector blind spot. The review and the detector agree that the surface is free of mechanical slop; every issue below is a behavioural or UX finding no scanner sees.

## Overall Impression
The peak (a preset, RUN, ladders climbing to exact values in ~200 ms) earns trust. The page does not yet protect that trust after the inputs change, and it lets destructive edits happen without a way back.

## What's Working
1. Values-as-returned is enforced in code: every figure carries `data-field` (JSON pointer) and `data-value` (the raw number); the raw exchange reads "Last run, byte for byte".
2. Amber carries meaning: one lit winner, returned values only, dark idle windows with screen-reader "not run yet" text.
3. Builder and raw JSON are two faithful paths, with native radio groups and visible bounds.

## Priority Issues
1. **[P1] Results go stale without any signal.** `src/ui/channels.rs:160-173,228-231`: after editing the state, an option or the model, the ladders keep the previous run's options and numbers beside editors that now say something else. Fix: compare the current request body with the last sent request; when they differ, mark the rail and the affected channels "From last run · inputs changed", drop the winner underline, light a "Changed" lamp by RUN; keep the figures exact.
2. **[P1] Destructive edits cannot be undone.** Presets overwrite pasted state without a guard (`src/ui/console.rs:36-39`, `src/deck.rs:114-118`); switching a question's type to noul drops its criteria (`src/builder.rs:157-159`); Remove question is instant (`src/ui/builder.rs:51-57`). Fix: a one-level snapshot before a preset load or removal with a "Restore previous" plate key; remember each question's criteria per type.
3. **[P2] Faults are not tied to their source.** The fault panel sits above all channels while the offending channel looks healthy; the parse error reads "EOF while parsing"; a disabled RUN gives no reason (`src/ui/channels.rs:93-142`, `src/ui/rail.rs:54`). Fix: mark the channel named by `loc`, `aria-describedby` on RUN, plain parse messages.
4. **[P2] On mobile, RUN and its result are a screen apart.** At 390x844 the result lands below the fold after RUN. Fix: scroll the first channel or the fault into view after a run below 720 px (instant under reduced motion).
5. **[P2] The default model is an unexplained alias.** "jev-latest (default model)" is not in `/v1/models` and the run reports "decider-2b-v11" (`src/ui/rail.rs:151-155`). Fix: label the alias with the model it resolves to.

## Persona Red Flags
- Power user: Ctrl/Cmd+Enter is undiscoverable; Tab leaves the JSON editor; a preset click wipes work.
- Screen reader: a successful run announces nothing (only faults are `role="alert"`); input tokens read twice; a disabled RUN has no reason.
- Stress tester: renaming an option after a run keeps the old name at 93.5%; choice to noul to choice loses options; tied score probabilities light several winners (`channels.rs:341`); RUN on the empty page can only 422.
- Mobile: presets wrap to two rows and Share link sits alone; the Received pane is a minified wall at 390 wide.

## Minor Observations
- Received is minified while Sent is pretty-printed.
- The output-tokens counter is always 0; input tokens appear twice.
- "Copied" never clears (`src/ui/controls.rs:58-72`).
- Amber focus ring around a Fault Ink frame on invalid JSON.
- The desktop cassette well is mostly empty for typical states.
- Confidence, P max, Certainty and Fit mass have no definition anywhere.

## Questions to Consider
- A "tape changed" lamp beside RUN that stays lit until the next run?
- A fault as a blown fuse on the channel that caused it, rather than a banner?
- Does an always-zero output counter earn a rail slot in a one-pass product?
- "Copy as curl" beside "Answered by", one click from the answer?
