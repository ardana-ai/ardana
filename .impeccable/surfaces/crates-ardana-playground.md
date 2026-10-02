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
- Developers wiring typed decisions into agents and automations; they arrive from ardana.ai (install, `ardana pull`, `ardana serve`) and move between the playground, their editor and a terminal.
- Task: state (text or JSON) on the left; `questions` map (noul, choice, score) plus results on the right (binding brand commitment); model picker from `/v1/models`; raw request/response; 413/422 `detail`; curl, Python and TypeScript snippets; three presets; Jev share links.
- Every displayed value equals the API response; probabilities as percent with one decimal, confidence, noul and score with two decimals.
- Behaviour, markup contract (roles, names, ids, test ids) and copy stay as they are: this round is a brand redesign only.
- Editors stay real, keyboard-first text areas; WCAG 2.2 AA (4.5:1 on all text, including on the ink Run pill); reduced motion honoured; light and dark follow the system.

## Direction contract
THESIS: The playground is ardana.ai at work: the same white paper, near-black ink and one gray, the groundhog lockup at the top-left, Onest for words and Geist Mono for whatever the API reads or returns. The state and every request sit in terminal-gray command boxes, questions are hairline rows like the model catalog, and the answer is the one ink row. It refuses the Notion workspace it replaces (warm ink, blue, gray panels, hover chips) and dashboard chrome (cards, shadows, gradients).
OWN-WORLD: The landing's tokens exactly: paper #ffffff, ink #0a0a0a, pencil gray #666666, hairline #ececec, terminal gray #f4f4f4 behind everything the API reads or returns (a box you type in adds a 1px code-gray ring; a box you copy from, the landing's command box, has none), selection as a 12% ink wash, a 2px ink focus ring 3px out. Onest Variable (15px UI, 13px labels, 700 headline at -0.035em) and Geist Mono Variable for code, model names and figures, both self-hosted. Ink pills for Run and the skip link, outline pills for tags and the checked segment, 10px code boxes with 6px inner keys, flat, 1px horizontal hairlines. No hue but one red, for faults only; dark is the brand kit's inverse (white logo and Run on #0a0a0a).
STORY: Coming from ardana.ai, the developer finds the same page: the groundhog top-left, the model and presets as quiet gray links, the state in a command box, questions as catalog rows. They press the ink Run pill or Ctrl+Enter, read each answer as an ink bar among gray ones with its exact figure in mono, and copy the request from Snippets as they copied `ardana pull`.
FIRST VIEWPORT: 1280x800. A 70px top row reads as the landing nav: the horizontal lockup at the landing's 26px, 22px from the top, at the top-left of a white 256px sidebar, then "Playground" in 15px ink, and at the right the "Inputs changed" outline pill, Share as a quiet text button and the 36px ink Run pill (one width at rest and while running, as the landing's Copy pill), one hairline beneath. Sidebar: 13px gray labels (Model, Presets, On this page), the model select as the landing's small command box (40px, ringed because it is picked) with the name in Geist Mono, preset and section rows as 15px gray links that turn ink on hover and when current. Page: "Ardana playground" in Onest 700 at about 40px with a 17px gray lede; then 5fr/7fr columns 40px apart: State as a ringed terminal-gray editor with the "Sent as text" outline pill, Questions as the landing's catalog rows (26px padding, 22px/600 id, outline-pill type control, gray instructions, 6px pill bars with mono figures where the answer is the one ink line and the other options speak in the gray, a fact row of 13px gray labels over values, 20px and 48px apart).
FORM: User-pinned: the Ardana landing world (../ardana-landing DESIGN.md "The Line", its brand kit and src/app.css tokens) carried into the playground at operating density. No concept roll: the world is pinned by the user and the structure by "no functional change"; the old Notion form (seed a4d7da67) is retired.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Craft bar
- ardana.ai itself (home, models list, model page) at its exact tokens; ollama.com and Vercel's dashboard for monochrome product density.

## Unresolved
- The landing draws hairlines horizontal only; the sidebar's right edge is the one vertical hairline, starting under the top row.
- The disclosure toggles open on a chevron: the landing keeps its arrow for navigation only (Restore previous).
