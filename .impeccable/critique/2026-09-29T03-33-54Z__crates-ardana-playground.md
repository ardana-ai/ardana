---
target: the Ardana playground
total_score: 28
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 4
target_identity: "file:/Users/tigran/projects/ardana/crates/ardana-playground"
timestamp: 2026-09-29T03-33-54Z
slug: crates-ardana-playground
---
# Critique: Ardana playground (Notion world, round 1)

Provenance: ⚠️ DEGRADED: single-context (the orchestrating session delegated this critique to one fresh-context reviewer; the design read, the browser drive and the detector ran in that one context, and the detector counts were read before the browser drive). The reviewer did not build the design and saw the two earlier critique records for their format only. Target `crates/ardana-playground`, live at `ardana serve` on 127.0.0.1:8123 with decider-2b (answers in 110 to 128 ms). Mode: Operate. Evidence: Playwright on the installed Chrome at 1280x800 and 390x844, light, plus 1280x800 dark; 53 screenshots and `log.json` in `tmp/evals/critique/`; `impeccable detect --json` on the empty, loaded, results and 422 URLs at both viewports (`tmp/evals/critique/detect/`). Every number below was measured in that run.

## Design Health Score
| # | Heuristic | Score | Key issue |
|---|---|---|---|
| 1 | Visibility of system status | 3 | An empty Run reads as a success ("Answered by decider-2b-v11 · 42 ms · 0 input"); stale is said four times |
| 2 | Match between system and real world | 3 | The 422 says "a map of 2..255 options" where the builder says "Options · 3 (2 to 255)" |
| 3 | User control and freedom | 3 | The Share popover ignores a click outside; Restore previous vanishes on the first keystroke |
| 4 | Consistency and standards | 3 | Toggle headings (20px) outrank the State and Questions headings (16px) |
| 5 | Error prevention | 2 | Run is live with no questions and no state; the JSON path is unguarded before the server |
| 6 | Recognition rather than recall | 3 | Every control is labelled; tooltips carry the shortcuts and the property meanings |
| 7 | Flexibility and efficiency of use | 3 | Ctrl+Enter, Ctrl+\, Escape, presets, share links; no keyboard jump between blocks |
| 8 | Aesthetic and minimalist design | 3 | Four stale tags, "0 output tokens" always, both toggles open on an empty page |
| 9 | Help users recognize, diagnose, recover from errors | 3 | The fault lands on its question, verbatim; on a phone its title hides under the top bar |
| 10 | Help and documentation | 2 | Tooltips and one callout; no link out to the API or the question types |
| | Total | 28/40 (Good) | |

## Design Specificity Verdict
Authored, and faithful to the reference. The 240px `#FBFBFA` sidebar with a workspace mark, the 45px hairline top bar with a breadcrumb, `Share` and a `#0075D3` Run, the 32px page title with its icon, the 14px system sans in `#37352F`, 6px radii, hover fills, the dark tooltip with a shortcut line, the `+ Add question` row and the toggle blocks are Notion's grammar played straight, as the brief asks. The product shows through the content, not the chrome: check-marked option rows with a 6px bar and the exact API figure right-aligned read as database properties; the `Sent as text` tag, the `Answered by decider-2b-v11 · 116 ms · 72 input` line and the byte-for-byte Raw exchange are Ardana's own. A Notion user pauses at four places: a native `<select>` for the model (Notion draws its own menu), a segmented control inside every block (Notion has no such control in a page), a popover that stays open when the page is clicked, and secondary toggle headings drawn larger than the primary block headings.

**Deterministic scan**: `impeccable detect --json` at 1280x800 found 1 finding on each of the empty, loaded and results URLs and 2 on the 422 URL; at 390x844 it found 0, 0, 0 and 1. Two rules fired: `line-length` ("~88 chars/line", 1280 only; the `.page-lede` and `.callout p` are capped at 70ch, so this is the detector's average-glyph estimate of a 70ch box: borderline, not acted on) and `low-contrast` on the 422 URL at both viewports: "3.9:1 (need 4.5:1), text #df5452 on #362422", the dark-scheme fault callout title. The browser measured the same pair at 3.86:1 (`dark.422.faultTitle`). Planted controls were not run this round; the scan is clean elsewhere, and the two hits are real.

**Visual overlays**: not attempted; the detector ran headless on the URLs and its findings are reported above with the file pointers below.

## Overall Impression
It looks like Notion, it behaves like Notion most of the way, and the numbers are honest. The shell is calm, the results block is the best thing on the page, and the keyboard story (Ctrl+Enter from the editor, Ctrl+\ for the sidebar, Escape with focus returned) is done properly. What breaks trust is at the edges: a Run that succeeds with nothing to answer, a phone that hides the error's headline under its own top bar, a Share popover cut off at the left edge of a phone, and a dark fault callout under AA. The biggest opportunity is guarding Run: the brief's banner "says why Run is held", and nothing holds it.

## What's Working
1. The results block. `billing` checked in blue at 93.5% with a 172px fill on a 184px track, the losers grey at 1px and 11px, then a property list whose names carry `<dfn>` tooltips ("The most probable option.") on hover and focus. Every figure carries `data-field` and `data-value`; the noul row keeps its two decimals as the brief asks.
2. The keyboard and focus discipline. Tab order runs skip link, sidebar (the hover-revealed Close sidebar button appears on focus), Share, Run, State, Add question, Questions JSON, the toggles; Ctrl+Enter inside the State textarea ran and left focus in the textarea; Escape closes the Share popover and the phone drawer and puts focus back on the button that opened them; the collapsed sidebar survives a reload.
3. The 422 lands where it belongs: a red callout at the top of the Questions column with the API's `body › questions › department` and its message, the faulted block outlined in red with the same message inside it, `Not answered: HTTP 422 · 1 ms` on the block head, `HTTP 422` on the Raw exchange, and the polite status region saying "HTTP 422, not answered".

## Priority Issues
1. **[P1] An empty Run is reported as a success.** `crates/ardana-playground/src/deck.rs:309-312` (`can_run` holds Run only for a JSON parse error or a run in flight). On the empty page at both viewports, clicking Run returned HTTP 200 and the page wrote "Answered by decider-2b-v11 · 42 ms · 0 input · 0 output tokens" beside a callout that still says "No questions yet", "0 input tokens in the last run" under the empty State, and the status region announced "Answered by decider-2b-v11: 0 questions". Nothing red, nothing held. Why it matters: the product's promise is that a developer trusts the numbers; a success line for a request that decided nothing teaches the opposite, and the brief's `#run-note` banner ("why Run is held") never appears. Fix: hold Run (`aria-disabled`) while `questions` is empty or the state is blank, and put the reason in the banner: "Add a question to run" / "Paste a state to run"; keep Ctrl+Enter silent under the same rule. Command: `$impeccable harden`.
2. **[P1] On a phone, whatever the page scrolls to hides under the sticky top bar.** `crates/ardana-playground/src/ui/mod.rs:261-285` (`scroll_to` with `block: Start`, `scroll_to_result`) and `src/ui/sidebar.rs:19-30` (`jump`), against `.topbar { position: sticky; min-height: 45px }` in `styles/shell.css:193-204`. Measured at 390x844: after the 422 the fault callout sits at y=0 with the 45px bar over it, so the first visible line is `body › questions › department` and the red title "HTTP 422 · the request was not answered." is off screen (`phone-07-422-view.png`); after a run the first question's `department` heading is clipped the same way (`phone-04-results-view.png`); "Raw exchange" from the sidebar lands at y=0 (`phone.toggle.jump`). At 1280 the same jump lands at y=44 under a 45px bar, flush with no breathing room. Why it matters: the moment the page moves the user, it covers the one line that explains where they are. Fix: `scroll-margin-top: calc(var(--topbar) + var(--space-3))` on `.question`, `.callout`, `.toggle` and `#content`, or scroll with an offset. Command: `$impeccable adapt`.
3. **[P1] The Share popover is cut off on a phone.** `styles/shell.css:280-293` (`.popover { right: 0; width: min(420px, calc(100vw - 24px)) }` anchored to the Share button). Measured at 390x844: the popover is 366px wide at x = -42, so its first 42px are off the left edge; the label reads "to these inputs", the input's left end and the note's first words are gone (`phone-10-share-popover.png`). Why it matters: Share is one of the four top-bar controls and this is the state a phone user sees first. Fix: below 720px make the popover `position: fixed; left: 12px; right: 12px; top: calc(var(--topbar) + 6px)` or anchor it to the top bar rather than the button. Command: `$impeccable adapt`.
4. **[P1] The dark fault callout is under AA.** `styles/base.css:92-94` (`--danger: #df5452`, `--danger-tint: #362422`, `--danger-tint-2`) with `styles/page.css:449-478` (`.callout-danger`, `.fault-title`, `.fault-loc`). Measured in the browser: `.fault-title` #df5452 on #362422 = 3.86:1 (the detector says 3.9:1); computed from the tokens, `.fault-loc` on `--danger-tint-2` over the callout is 2.93:1; the dark `.banner` uses the same pair. Light is fine (5.21:1 title, 4.55:1 loc). Why it matters: the brief says 4.5:1 on all text, and the fault title is 14px at weight 600, not large text. Fix: in the dark scheme lighten the danger ink for text on tints (#ef7a77 computes to 5.4:1 on #362422; #2a1c1b under the current red reaches only 4.32:1, so the tint alone cannot fix it), and give `.fault-loc` the callout background instead of `--danger-tint-2`. Command: `$impeccable colorize`.
5. **[P2] Stale is said four times and moves the page.** `src/ui/topbar.rs:50-56`, `src/ui/questions.rs:110-116` and `:330-336`, `styles/page.css:61-68`. After one edit at 1280 the page shows "Inputs changed since the last run" in the top bar, "From last run · inputs changed" on the block head and again inside each of the two question blocks; the Questions block head grew from 28px to 70px as its last-run line wrapped under the heading, pushing both cards down 42px. Fix: keep the top-bar tag and the grey bars and marks (already done: the blue fill turns `#787774`), drop the per-question tags, and give `.last-run` its own row under the head so nothing reflows. Command: `$impeccable distill`.
6. **[P2] The Share popover does not close on a click outside.** `src/ui/topbar.rs:94-162`: only the button and Escape toggle it. Clicking the page at both viewports left `#share-panel` open. Notion, Linear and GitHub all dismiss a popover on an outside click. Fix: a document `pointerdown` listener while open, or a native `popover` attribute. Command: `$impeccable polish`.
7. **[P2] The type scale puts the secondary blocks above the primary ones.** `styles/page.css:635-639` (`.toggle-heading` 20px/600) against `.block-heading` and `.question-id` (16px/600, `page.css:70-74`, `207-214`). "Raw exchange" and "Snippets" read larger than "State" and "Questions" although they are the page's appendices. Notion's toggle heading is body size. Fix: toggle headings at 16px/600 like the block headings, or promote State/Questions to 20px. Command: `$impeccable typeset`.
8. **[P2] The 422 names a constraint in the API's words and stops.** `src/ui/questions.rs:166-218`, `:337-352`. "choice criteria: a map of 2..255 options" is verbatim (a product principle) but the builder calls the same thing "Options · 3 (2 to 255)" and the JSON holds an array, so the user must translate; no hint says "add an option" or "press Edit". Fix: keep the verbatim line and add one playground line under it: "Open Edit and add an option (2 to 255)." Command: `$impeccable clarify`.

## Persona Red Flags
- **Alex (power user)**: no red flags on the desktop path; Ctrl+Enter, Ctrl+\ and Escape all work and the tooltips print the shortcuts. Friction: no key to jump from a question to its JSON or the snippet; "Restore previous" disappears the moment a character is typed, so a preset loaded by mistake cannot be undone after a first edit.
- **Sam (screen reader, keyboard)**: the empty Run announces "Answered by decider-2b-v11: 0 questions" as a success; the stale state is carried by four identical tags in the reading order; the dark fault title is 3.86:1; the hover-only Close sidebar button is fine because focus reveals it.
- **Jordan (first-timer)**: the empty callout is a 51-word paragraph with three code words; the blue Run invites a click with nothing filled in and the page congratulates them; "noul" is defined only in that callout, and Confidence, P max and Certainty only in tooltips the newcomer must discover.
- **Casey (phone)**: Run is in the sticky bar so it is always reachable, but every landing (results, fault, sidebar jump) hides its headline under that bar; the Share popover loses its left 42px; the drawer promises "Ctrl+Enter or ⌘↵ runs" on a touch screen; touch targets are 24px (Close sidebar, Edit, segments, Copy) to 28px (rows, Run, Share), none at 44.

## Minor Observations
- "0 output tokens" is printed after every answered run; a System 1 model never emits tokens, so the figure is noise on the one line that should be tight.
- The empty callout runs four lines at 1280 and six at 390; Notion callouts carry one or two.
- Both toggle blocks open by default, so an empty page already shows an empty Raw exchange and a curl snippet for a request with an empty state; the page is 1281px tall before anything is typed.
- Heading outline: `h3` "Sent · POST /v1/systemone" and "Received" sit inside an `h3` toggle, and `h4` question ids sit under an `h3` "Questions"; the toggles' inner labels should be `h4` or plain labels.
- noul reads 0.93 while choice reads 93.5% (the brief's contract, but the mixed formats sit two rows apart).
- The idle readout dash is `--ink-3` at 2.49:1; it carries a hidden "not run yet", so it passes as decoration, but a lighter en dash would look less like a missing value.
- The Copy toast is fixed at bottom right and reads "Copied the share link"; good, and it also appears when copying a snippet.
- The Model select lists "decider-4b · 2.7 GB" under "Library · pulls on first run", which is the right place for the size; the "Pulled" group shows sandbox models (llama3.2, smollm3-3b) whose names a newcomer cannot tell apart from decider models.

## Questions to Consider
- Should Run be a verb that can be greyed, with the banner saying why, so a success line can only ever follow a real decision?
- Could the stale state live in one place (the top-bar tag plus grey bars) and nowhere else?
- Should the phone's landings be offset by the bar height everywhere the page moves the user, and the Share popover become a sheet under the bar?
- Do the Raw exchange and Snippets belong open by default, or closed until the first run, as Notion's toggles usually start?
- Would a dark danger ink chosen for 4.5:1 on its tint let the same red serve the banner, the callout and the code chip?
