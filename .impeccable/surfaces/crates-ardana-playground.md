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
- Developers wiring typed decisions into agents and automations; they live in Notion, Linear and GitHub all day.
- Task: state (text or JSON) on the left; `questions` map (noul, choice, score) plus results on the right (binding brand commitment); model picker from `/v1/models`; raw request/response; 413/422 `detail`; curl, Python and TypeScript snippets; three presets; Jev share links.
- Every displayed value equals the API response; probabilities as percent with one decimal, confidence, noul and score with two decimals.
- Editors stay real, keyboard-first text areas; WCAG 2.2 AA (4.5:1 on all text, including on the blue button); reduced motion honoured; light and dark follow the system.

## Direction contract
THESIS: The playground is a Notion workspace page: a quiet sidebar of pages and settings beside one white page whose blocks are the work, where calibrated numbers read like database properties with progress bars. It refuses the instrument-panel metaphor (engraved legends, glowing readouts) and the neon developer console alike.
OWN-WORLD: Notion's app grammar played straight: the system sans at 14px, warm ink #37352F on white with a #FBFBFA sidebar, #E9E9E7 hairlines, 4 to 6px radii, hover fills at 8% ink, one blue (#2383E2, #0075D3 as a fill) for the primary action, focus, selection and the winning answer, red only inside fault callouts; toggle blocks, callouts, property rows, progress-bar properties, gray tags, dark tooltips with shortcuts; the dark scheme follows the system (#191919 page, #202020 sidebar).
STORY: The developer opens what looks like a workspace they already know, picks a model and a preset in the sidebar, edits the state and the questions as blocks, presses the blue Run in the top bar, reads every option's probability as a property bar with its exact value, and copies the request from the Snippets toggle.
FIRST VIEWPORT: Left: a 240px sidebar (Ardana, the Model select, Presets as page rows, the page's sections). Right: a sticky 45px top bar (page title, the stale tag, Share, the blue Run with its shortcut tooltip) over the page title "Playground" and two columns: the State code block left (5fr) with its sent-as tag and token caption, the question blocks right (7fr), each with its id, instructions, a type segmented control, Edit, option rows with bars and exact values, a property list of answer and confidence, then a "+ Add question" row and the Questions JSON code block.
FORM: The category standard taken at the standing exit: Notion's app UI executed at full fidelity, Linear and GitHub as craft peers; the roll's assigned grounded candidate 3 and its dealt challengers were set aside by the user-pinned brief; seed key a4d7da67.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Craft bar
- Notion's app (the workspace: sidebar, page, database properties, toggles, callouts), Linear (dense calm product UI), GitHub (settings pages and code views).

## Unresolved
- Below 960px the sidebar is a drawer behind a menu button in the top bar; presets and the model select live inside it.
