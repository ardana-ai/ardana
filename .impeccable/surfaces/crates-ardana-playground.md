---
version: 1
slug: "crates-ardana-playground"
primary_target: "crates/ardana-playground"
related_targets: []
---

# Surface brief: Ardana playground

## Scope and mode
- Target: `crates/ardana-playground` (Leptos CSR, served by `ardana serve` at `/`).
- Mode: Operate. The visitor completes a task: paste a state, build questions, run them against a local model, read calibrated answers, copy the request into code.

## Audience, task, content, constraints
- Developers wiring typed decisions into agents and automations.
- Task: state (text or JSON) on the left; `questions` map (noul, choice, score) plus results on the right (binding brand commitment); model picker from `/v1/models`; raw request/response; 413/422 `detail`; curl, Python and TypeScript snippets; three presets; Jev share links.
- Every displayed value equals the API response; probabilities as percent with one decimal, confidence, noul and score with two decimals.
- Editors stay real, keyboard-first text areas; WCAG 2.2 AA; reduced motion honoured.

## Direction contract
THESIS: The playground is one tape-deck fascia: state loads like a cassette, each question is a channel, and every option's probability climbs an amber LED ladder. It refuses the two-pane API console of grey borders and a blue Run button.
OWN-WORLD: Brushed-aluminium panel on a beige plastic surround, engraved black legends in narrow grotesk caps under every control, chrome-toggle model and type switches, milled tracks, amber LED ladders and dot-matrix amber numerals reserved for live figures; red reserved for errors only.
STORY: The developer loads a state, sets each channel's type and criteria, presses RUN, and watches the ladders settle on the exact numbers the API returned, then lifts the request as a snippet.
FIRST VIEWPORT: Left: the state in a cassette window (a real text area) whose counter shows `usage.input_tokens` after each run. Right: bolted channel rows, one per question: engraved id, type toggle, option ladders with exact percent readouts, the winning answer lit. Top rail: model switch, RUN key at the far right, latency and token counters in amber.
FORM: Cassette-futurism tape deck fascia, dealt challenger (competitive; grounded list position n/a), seed key 32c4d5ab. Signature interaction: RUN transport key; ladders climb to their returned values (static under reduced motion). Motion grammar: toggle snap and ladder climb only.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Craft bar
- QUALITY BAR board: https://impeccable.style/worlds/cards/pop-culture-shelf-cassette-futurism-deck.webp
- Hero: https://impeccable.style/worlds/cards/pop-culture-shelf-cassette-futurism-deck-hero.webp

## Unresolved
- Mobile (390 wide): the fascia restacks into narrow bolted panels, state first, channels below.
