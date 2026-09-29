---
target: the Ardana playground
total_score: 29
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 0
target_identity: "file:/Users/tigran/projects/ardana/crates/ardana-playground"
timestamp: 2026-09-29T03-42-57Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (Notion world, round 2)

Provenance: ⚠️ DEGRADED: single-context (the same fresh-context reviewer as round 1 re-verified the rebuilt server in one context; the detector ran after the browser drive this time). Target `crates/ardana-playground`, live at `ardana serve` on 127.0.0.1:8123 with decider-2b. Mode: Operate. Evidence: Playwright on the installed Chrome at 1280x800 and 390x844 in light, and the 422 state in dark at both viewports, every navigation a full load; screenshots in `tmp/evals/critique/shots2/`, measurements in `tmp/evals/critique/log2.json`; `impeccable detect --json` on the empty, loaded, results and 422 URLs at both viewports (`tmp/evals/critique/detect2/`).

## Round history
Round 1 (2026-09-29T03-33-54Z, 28/40, 4 P1) named: (1) an empty Run reported as a success; (2) phone landings hidden under the sticky top bar; (3) the Share popover clipped 42px off the left of a phone; (4) the dark fault callout at 3.86:1. All four are closed in this build, each re-measured below. Of round 1's P2s, the toggle-heading type scale is fixed (16px), the property list is one property per row, noul rows lost their checkbox, and the sidebar marks the section in view; the fourfold stale wording and the popover that ignores an outside click are unchanged by decision.

## Design Health Score
| # | Heuristic | Score | Key issue |
|---|---|---|---|
| 1 | Visibility of system status | 3 | Held Run now explains itself; stale is still said four times |
| 2 | Match between system and real world | 3 | "a map of 2..255 options" against the builder's "Options · 3 (2 to 255)" |
| 3 | User control and freedom | 3 | The Share popover closes on Escape or its button only |
| 4 | Consistency and standards | 3 | Type scale repaired; noul 0.93 beside choice 93.5% remains |
| 5 | Error prevention | 3 | Run held with a reason for no questions and for a JSON error; a blank state still runs |
| 6 | Recognition rather than recall | 3 | `aria-current` on the "On this page" rows; property meanings only in tooltips |
| 7 | Flexibility and efficiency of use | 3 | Unchanged |
| 8 | Aesthetic and minimalist design | 3 | Four stale tags, "0 output tokens", both toggles open on an empty page |
| 9 | Help users recognize, diagnose, recover from errors | 3 | The fault's title is visible everywhere now; still no playground hint under the verbatim message |
| 10 | Help and documentation | 2 | Unchanged |
| | Total | 29/40 (Good) | |

## P1 re-verification
1. **Empty Run held.** At both viewports the empty page shows `.run-key[aria-disabled="true"]` and the banner "Add a question to run" in `.banner.quiet` (ink #37352F on #FBFBFA, 11.84:1). A forced click on Run and Ctrl+Enter inside the State textarea produced no run: `.last-run` count 0, the status region empty, no Raw exchange status. Clicking "+ Add question" released Run (`aria-disabled="false"`, banner empty); pasting `{ nope` into Questions JSON held it again with the red banner "Questions JSON has an error" (#B3372F on #FDEBEC). The loaded share link opens with Run released. Closed.
2. **Landings clear the top bar.** `.question` reports `scroll-margin-top: 57px` (45 + 12). At 390x844 after a run the first question sits at y=57 (scrollY 511); after the 422 the callout sits at y=57 and its title "HTTP 422 · the request was not answered." at y=73, fully visible (`phone-03-422.png`); "Raw exchange" from the sidebar lands at y=57 at both viewports, and the row takes `aria-current`. At 1280 the results and fault stay at y=237 with no scroll. Closed.
3. **Share popover on a phone.** `#share-panel` is `position: fixed`, x=12 to 378 of 390, y=51 under the 45px bar, label "Link to these inputs" whole, focus in the link input (`phone-05-share.png`). At 1280 it is unchanged: absolute under the button, x=794 to 1214. Closed.
4. **Dark fault callout.** `--danger` is #f28b88; measured in the browser at both viewports: `.fault-title` 6.16:1 on #362422, `.fault-loc` 5.23:1, the dark JSON-error banner 6.16:1, the question's inline fault text 10.1:1. Light is unchanged at 5.21:1 and 4.55:1. Closed. One note: `.fault-loc`'s measured own background is `--hover` (rgba(255,255,255,0.055)), not the intended rgb(0 0 0 / 0.3), because `.callout code` at `styles/page.css:454` (specificity 0,1,1) outranks `.fault-loc` at `:484` (0,1,0); the chip passes anyway.

**Deterministic scan**: 1280x800: 1 finding on each of the empty, loaded, results and 422 URLs, all `line-length` ("~88 chars/line", the 70ch lede and callout measured in average glyphs; borderline, not acted on). 390x844: 0, 0, 0, 0. The round-1 `low-contrast` finding on the 422 URL is gone at both viewports.

## What's Working
1. The held Run reads as Notion would do it: the blue button at half strength, one quiet grey line under the bar saying what to do, red reserved for a broken JSON document.
2. The property list as one row per property (name in a 9rem column, value beside it) reads as a Notion property block; the noul row without a checkbox no longer suggests a choice that was not made.
3. Every landing the page performs now leaves 12px between the sticky bar and the thing it scrolled to, on both viewports.

## Priority Issues
1. **[P2] Stale is said four times and the block head reflows.** `src/ui/topbar.rs:50-56`, `src/ui/questions.rs:110-116, 330-336`; kept by decision for the tests. Fix when the tests can move: one top-bar tag plus the grey bars, and `.last-run` on its own row.
2. **[P2] The Share popover ignores a click outside.** `src/ui/topbar.rs:94-162`; measured still open after a click on the page at both viewports. Fix: a document `pointerdown` listener while open, or the native `popover` attribute.
3. **[P2] The 422 stops at the API's words.** `src/ui/questions.rs:166-218`: "choice criteria: a map of 2..255 options" with no line pointing at Edit → Add option. Fix: one playground line under the verbatim message.
4. **[P2] Touch targets are 24 to 28px on a phone** (Close sidebar 24, Edit 24, segments 24, Copy 24; rows, Run and Share 28). Fix: a 44px hit area via padding or `::before` at ≤720px without changing the drawn size.
5. **[P2] A blank state still runs.** `can_run` holds for an empty questions map only; a question with an empty state is sent. Fix: the same hold with "Paste a state to run", if the product wants it (the API accepts an empty string, so this is a judgement call).
6. **[P3] `.fault-loc` dark background rule is dead**: `styles/page.css:484` loses to `.callout code` at `:454`. Fix: `.callout .fault-loc` or move the rule below with equal specificity.

## Persona Red Flags
- **Jordan (first-timer)**: the empty page now tells them what to do and refuses a pointless run; the four-line callout and the "noul/choice/score" jargon remain the first thing they read.
- **Casey (phone)**: results, faults and section jumps now land below the bar, and Share opens as a sheet under it; the 24px controls and the "Ctrl+Enter or ⌘↵ runs" line in the drawer remain.
- **Sam (screen reader)**: the held Run keeps focus (`aria-disabled`) and is described by the banner via `aria-describedby`; the stale state still repeats four times in the reading order.

## Minor Observations
- In the results screenshot the property names no longer show the dotted `<dfn>` underline round 1 had; the hidden meaning text is still present in each row.
- "0 output tokens" remains on every answered run.
- Both toggle blocks still open by default on an empty page.
- noul reads 0.93 while choice reads 93.5% (the brief's contract).

## Questions to Consider
- With Run held for no questions, should a blank state hold it too, or is an empty state a legitimate probe of the questions alone?
- Can the stale signal collapse to one place once the tests are rewritten around the top-bar tag?
- Should the popover become a native `popover` so light dismiss comes for free?
