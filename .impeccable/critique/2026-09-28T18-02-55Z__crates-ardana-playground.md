---
target: the Ardana playground
total_score: 28
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 0
target_identity: "file:/Users/tigran/projects/ardana/crates/ardana-playground"
timestamp: 2026-09-28T18-02-55Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (round 2, after the last UI fix)

Provenance: dual-agent run. Assessment A (design review) and Assessment B (detector and browser evidence) ran as two isolated fresh-context sub-agents after the final W7 fix batch; A saw no detector output and no earlier critique. Target `crates/ardana-playground`, live at a local `ardana serve` with decider-2b and qwen3.5-0.8b. Mode: Operate.

## Design Health Score
| # | Heuristic | Score |
|---|---|---|
| 1 | Visibility of system status | 3 |
| 2 | Match between system and real world | 3 |
| 3 | User control and freedom | 3 |
| 4 | Consistency and standards | 3 |
| 5 | Error prevention | 2 |
| 6 | Recognition rather than recall | 3 |
| 7 | Flexibility and efficiency of use | 3 |
| 8 | Aesthetic and minimalist design | 3 |
| 9 | Help users recognize, diagnose, recover from errors | 3 |
| 10 | Help and documentation | 2 |
| | Total | 28/40 (Good) |

## Design Specificity Verdict
Authored for this product: cassette window for the state, a bolted channel per question, ladders climbing to the returned probability, amber only on returned figures, red only on the 422; channels, ladders and the Changed lamp each map to an Ardana concept. Weak point: stale ladders keep full amber, diluting "amber means live".

**Deterministic scan**: `impeccable detect` returned 0 findings on `index.html` and on `/`, the loaded share link, the autorun results and a 422 URL at 1280x800 and 390x844 (also `--no-config`); the in-page overlay reported no anti-patterns on empty, results and builder-open at both viewports. Planted controls fired (22 file findings, 15 URL findings, 11 overlay findings inside the live Ardana page), so the clean result is not a blind spot. The detector cannot see the one type-scale inversion A-adjacent evidence noted (question h3 20px above section h2 18px); it is recorded as a minor observation.

## Overall Impression
Every figure on screen equals the API response; the 422 lands on the right channel; status is announced; nothing blocks the task. What remains is polish around error prevention, definitions of the returned figures, and phone ergonomics.

## What's Working
1. Faithful numbers tied to their JSON pointers; idle windows empty rather than fake zeros.
2. Pre-drawn dark ladders after a preset load show the answer's shape before the run.
3. Accessibility done properly: native radios, `aria-disabled` RUN keeping focus, polite status regions, ink focus rings on aluminium, results scrolled into view on phones.

## Priority Issues
1. **[P2] Stale answers still glow as live signal.** `styles/deck.css:273`, `src/ui/channels.rs:233-243`, `src/ui/rail.rs:207-217`: only the winner underline changes. Fix: dim ladder fills and readouts (amber-dim) under `.channel.stale` and on the rail while the Changed lamp is lit, numbers unchanged.
2. **[P2] No warning before a request the page knows will be refused.** The JSON path ignores the builder's bounds; the empty page offers a ready-looking RUN. Fix: quiet preflight notes on the channel, a playground hint under the verbatim 422 text, inert RUN when state and questions are empty.
3. **[P2] On a phone, RUN is out of reach of editing.** RUN sits at the top of a ~3100 px page. Fix: a bottom transport bar at ≤720 px or a RUN key beside Add question.
4. **[P2] Returned figures are never defined.** Confidence, P max, Certainty, Fit mass (`src/ui/channels.rs:344-409`). Fix: `<dfn>` legends with one-line descriptions and an API reference link.
5. **[P3] The same signal is shown two or three times.** The 422 message twice, Input tokens twice, the stale note on the rail and every channel. Fix: one of each per location.

## Persona Red Flags
- Power user: Ctrl/Cmd+Enter not shown on RUN; no keyboard jump from a channel to the JSON or snippet; Restore previous disappears after the next edit.
- Screen reader: stale is visual-only on channels (ladders are `aria-hidden`).
- First-timer: "noul", "P max", "Certainty" undefined; the 422 says "map" where an array was written; noul reads 0.93 while choice reads 93.5%.
- Phone: RUN at the top, results a screen down, three always-open glass panes.

## Minor Observations
- "Copied" sits past the snippets bar's right edge at 1280.
- Output tokens always 0 in a large amber window.
- The questions placeholder starts with "Example:", not valid JSON if pasted.
- Latency stays lit after a 422 while token counters go dark.
- Ladder ticks are faint at phone width.
- Question h3 (20px) sits above section h2 (18px) in size.

## Questions to Consider
- Should a stale ladder power down to the unlit colours with its numbers kept?
- Should the Questions JSON be a service panel behind the channels, open only when the builder cannot represent a question?
- A transport bar that travels with the user on a phone?
- Should noul read as a percent like every other probability?
- A channel lamp that will not light until the question is valid?
