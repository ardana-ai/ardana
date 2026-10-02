---
target: the Ardana playground
total_score: 29
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 0
target_identity: "file:/Users/tigran/projects/ardana-ai/ardana/crates/ardana-playground"
timestamp: 2026-10-01T23-57-50Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (ardana.ai brand world, round 2)

Method: dual-agent (A: fresh-context design review · B: fresh-context detector and browser evidence), parallel and isolated; A wrote its unanchored assessment before it opened round 1's issue list. Target `crates/ardana-playground`, live at `ardana serve` on 127.0.0.1:8712 with decider-2b (served CSS diffed identical to `styles/`), compared with the landing page's own build. Mode: Operate. Evidence: Playwright on the installed Chrome at 1280x800, 1024, 1100, 390x844 and 320, light and dark, CDP platform fonts (`tmp/evals/critique-r2/a/`); `impeccable detect --json` on `index.html`, the crate, and the empty, loaded, results and 422 URLs at both viewports in both colour schemes (32 URL scans, each proved by an injected canary), the in-page overlay on results and 422 (`tmp/evals/critique-r2/b/`).

## Round history
Round 1 (2026-10-01T23-02-41Z, 28/40, P1 2) named: (1) Geist Mono never loaded; (2) the 422 location chip at 4.47:1. Both are closed: CDP draws every mono string in Geist Mono (web font), the same face as the landing's command boxes, and a test line measures 327.61px against Geist Mono's 327.60 (Menlo 328.72); the chip is `#b42318` on `#f9dcd9` at 5.09:1 (5.62:1 dark). Of round 1's P2s, the answer as the one ink line is fixed (other options `#666` at 400, the answer ink at 600, a noul's bar ink); empty wire panes are plain sentences; editable boxes carry a 1px code-gray ring that tells them from copy-only boxes (sanctioned by the brief); the disclosure arrow is a chevron; the dark logo and Run are white; the current row is ink at 400; the share tooltip no longer covers the note. Partly open after this round: the score fit figure at desktop widths and the state box's visual weight.

## Design Health Score
| # | Heuristic | Score | Key issue |
|---|---|---|---|
| 1 | Visibility of system status | 3 | "Running ▌" and the status region; the held-Run reason sits in a banner that scrolls away; on phones autorun scrolls the model and latency line out of view |
| 2 | Match between system and real world | 3 | Developer vocabulary throughout; noul, P max and Fit mass explained only in definitions and one empty-state sentence |
| 3 | User control and freedom | 3 | Escape, scrim and Restore previous work; the undo for Remove question is a sidebar row, in the drawer on phones |
| 4 | Consistency and standards | 3 | One ring, one pill, one command box like the landing; the Run pill changed width while running; legends looked like options |
| 5 | Error prevention | 3 | Run held on invalid JSON with line and column; library models show their size before a pull |
| 6 | Recognition rather than recall | 3 | Labelled on desktop; the sidebar's toggles are icon-only, their tooltips only where a pointer hovers |
| 7 | Flexibility and efficiency of use | 3 | Ctrl/⌘+Enter, Ctrl/⌘+\, share links, autorun, three snippet languages; 4–5 extra tab stops per answer |
| 8 | Aesthetic and minimalist design | 3 | Genuinely restrained; one "inputs changed" message four times after an edit |
| 9 | Help users recognize, diagnose, recover from errors | 3 | The 422 names `body › questions › department` twice and shows the API's message |
| 10 | Help and documentation | 2 | Primer and definitions only; no route to criteria docs |
| **Total** | | **29/40** | **Good** |

## Design Specificity Verdict
**LLM assessment:** authored for Ardana, not interchangeable. Font files, logo path data and favicons are byte-identical to the landing's; the tag pill, copy key, `$` prompt, skip link, selection wash and fact-row type are exact; a computed-colour scan of every element across four states and two schemes finds no hue but the fault red in the 422 state. Product-specific moves: the answer as the one ink line among gray ones, the landing's caret as the busy signal, the curl snippet's gray `$`, a JSON pointer on every figure. The skeleton is the category's, by binding commitment.

**Deterministic scan:** `index.html` clean; the crate directory gave six `overused-font` warnings for the Geist Mono faces, the brand's pinned mono, waived in `.impeccable/config.json` with the user's confirmation as the reason (0 after the waiver); all 32 URL scans exit 0 with no findings, before and after the DESIGN.md rewrite; the light-scheme 422 finding of round 1 is gone.

**Visual overlays:** injected into results and 422 at both viewports and both schemes: "No anti-patterns found" in all eight; a deliberately bad control page reported five, so the overlay works here. Platform fonts identical across all 16 contexts: Onest for the heading, Geist Mono for the select, readouts and snippet, no system fallback.

## Overall Impression
It now reads as ardana.ai at work, down to the bytes of its type and logo. What remains is operational polish rather than brand drift: rows that cramp beside the sidebar on small laptops, a Run pill that changed width, a stale message said four times.

## What's Working
1. Token-exact brand: fonts, logo, favicons, tag pill, copy key, prompt, skip link and selection measure identical to the landing's.
2. The answer reads as the one ink line, with its figure in Geist Mono and its JSON pointer; the dark inverse is equally clean.
3. Keyboard and accessibility craft: the 2px ring 3px out at all 29 stops, tooltips on focus, an inert page under the drawer, the status region, no sideways scroll at 320.

## Priority Issues
- **[P2] The answer bars collapse at 961–1180px.** The fixed name column leaves a 27–145px track beside the sidebar. Fix: stack the bar under name and figure when the questions column is narrow (a container query). Suggested command: $impeccable adapt
- **[P2] Editing after a run shoves the answers 63px and says "inputs changed" four times.** Fix: per-question tags only for the question that changed, the column note inline. (Behaviour unchanged by decision this round.) Suggested command: $impeccable distill
- **[P2] The Run pill changes width on every run** (64 → 112px), unlike the landing's Copy pill. Fix: stack the faces in one cell with the inactive ones hidden. Suggested command: $impeccable polish
- **[P2] On phones the model and presets hide behind an unlabelled ».** Fix: a panel glyph or a visible label for the opener. Suggested command: $impeccable clarify
- **[P3] The top row and type scale drift from ardana.ai** (64px row against 70, 20px row titles against 22, fact gaps 16/40 against 20/48, the phone lockup at 22px). Suggested command: $impeccable typeset

## Persona Red Flags
**Alex (power user):** Run is the 11th tab stop; the Run pill twitches each run; neither raw pane has a copy key; the snippet language resets on reload.
**Sam (accessibility):** "Type" and "Language" legends styled exactly like their options; a held Run shown by fading, its reason in a banner that scrolls away; a focused fact tooltip covers the row above.
**Casey (mobile):** model and presets behind »; autorun scrolls the "Answered by" line away; 28px type segments.
**Priya (integrator):** the picker says `decider-2b`, the answer `decider-2b-v11`; "0 output tokens" unexplained.

## Minor Observations
- The state placeholder (words) is set in Geist Mono; the landing sets words in Onest.
- `<title>` "Ardana playground" breaks the site's "Page · Ardana" pattern.
- The back arrow's stroke is 1.5 against the landing's 1.6; the noul answer's figure is 400 where a choice answer's is 600.
- The share popover is flat white over white with a 1.18:1 hairline.
- The picker shows "563.0 MB" where the landing shows "563 MB" (the shared size formatter).

## Questions to Consider
1. What if the empty page opened on a preset with Run already ink, one Ctrl+Enter from an answer, as the landing is one click from the clipboard?
2. Should staleness be one note for the answer column rather than a pill on every row?
3. Could the answer line be this page's display moment, the way the install command is the landing's?
