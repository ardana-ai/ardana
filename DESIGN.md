---
name: Ardana
description: The playground is ardana.ai at work; white paper, near-black ink, one gray, Geist Mono for whatever the API reads or returns, and each answer the one ink line.
colors:
  bg: "#ffffff"
  ink: "#0a0a0a"
  muted: "#666666"
  line: "#ececec"
  accent: "#0a0a0a"
  accent-hover: "#2b2b2b"
  on-accent: "#ffffff"
  code-bg: "#f4f4f4"
  code-ink: "#0a0a0a"
  code-muted: "#8a8a8a"
  code-hover: "rgb(10 10 10 / 0.07)"
  focus: "#0a0a0a"
  selection: "rgb(10 10 10 / 0.12)"
  track: "#ececec"
  bar: "#858585"
  scrim: "rgb(10 10 10 / 0.4)"
  danger: "#b42318"
  danger-tint: "#fdf0ef"
  danger-tint-2: "#f9dcd9"
typography:
  headline-page:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "clamp(2rem, 3.2vw, 2.75rem)"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.035em"
  headline-page-compact:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1.875rem"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "-0.035em"
  headline-notice:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "clamp(1.75rem, 3vw, 2.25rem)"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "-0.03em"
  title:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1.375rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  title-compact:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  lede:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1.0625rem"
    fontWeight: 400
    lineHeight: 1.45
  body:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.9375rem"
    fontWeight: 400
    lineHeight: 1.5
    fontFeature: "tnum"
  button:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.9375rem"
    fontWeight: 600
    lineHeight: 1.2
  link:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.9375rem"
    fontWeight: 400
    lineHeight: 1.2
  small:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.4
  label:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.8125rem"
    fontWeight: 500
    lineHeight: 1.4
  caption:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.4
  pill:
    fontFamily: '"Onest Variable", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0"
  code-block:
    fontFamily: '"Geist Mono Variable", ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.6
  code-sm:
    fontFamily: '"Geist Mono Variable", ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.45
  code-cell:
    fontFamily: '"Geist Mono Variable", ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: "0.875rem"
    fontWeight: 400
    fontFeature: "tnum"
  code-xs:
    fontFamily: '"Geist Mono Variable", ui-monospace, "SF Mono", Menlo, monospace'
    fontSize: "0.75rem"
    fontWeight: 400
rounded:
  focus: "3px"
  code: "4px"
  inner: "6px"
  box: "10px"
  tip: "12px"
  pill: "999px"
spacing:
  space-1: "4px"
  space-2: "8px"
  space-3: "12px"
  space-4: "16px"
  space-5: "24px"
  space-7: "40px"
  space-8: "48px"
  row: "26px"
  gutter: "clamp(16px, 4vw, 48px)"
components:
  button:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.link}"
    rounded: "{rounded.inner}"
    padding: "0 10px"
    height: "32px"
  button-hover:
    textColor: "{colors.ink}"
  button-sm:
    padding: "0 8px"
    height: "28px"
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.button}"
    rounded: "{rounded.pill}"
    padding: "0 18px"
    height: "36px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
    textColor: "{colors.on-accent}"
  button-primary-compact:
    padding: "0 14px"
  button-icon:
    backgroundColor: "transparent"
    textColor: "{colors.code-muted}"
    rounded: "{rounded.inner}"
    size: "32px"
  button-icon-hover:
    backgroundColor: "{colors.code-hover}"
    textColor: "{colors.code-ink}"
  copy-key:
    backgroundColor: "transparent"
    textColor: "{colors.code-muted}"
    rounded: "{rounded.inner}"
    size: "40px"
  copy-key-hover:
    backgroundColor: "{colors.code-hover}"
    textColor: "{colors.code-ink}"
  segment:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    padding: "0 12px"
    height: "28px"
  segment-hover:
    textColor: "{colors.ink}"
  segment-on:
    textColor: "{colors.ink}"
  segmented-legend:
    textColor: "{colors.muted}"
    typography: "{typography.caption}"
  tag:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.pill}"
    rounded: "{rounded.pill}"
    padding: "2px 8px"
  field:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-ink}"
    typography: "{typography.body}"
    rounded: "{rounded.box}"
    padding: "6px 12px"
    height: "36px"
  field-label:
    textColor: "{colors.muted}"
    typography: "{typography.label}"
  field-note:
    textColor: "{colors.muted}"
    typography: "{typography.caption}"
  field-fault:
    textColor: "{colors.danger}"
    typography: "{typography.caption}"
  field-disabled:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
  select:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-ink}"
    typography: "{typography.code-sm}"
    rounded: "{rounded.box}"
    padding: "0 36px 0 14px"
    height: "40px"
  command-box:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-ink}"
    typography: "{typography.code-block}"
    rounded: "{rounded.box}"
    padding: "16px 18px"
  command-row:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-ink}"
    typography: "{typography.code-sm}"
    rounded: "{rounded.box}"
    padding: "0 4px 0 14px"
    height: "40px"
  inline-code:
    backgroundColor: "{colors.code-bg}"
    textColor: "{colors.code-ink}"
    rounded: "{rounded.code}"
    padding: "0.1em 0.35em"
  sidebar:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.ink}"
    width: "256px"
  sidebar-heading:
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    padding: "0 12px"
    height: "28px"
  nav-row:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.link}"
    rounded: "{rounded.inner}"
    padding: "0 12px"
    height: "34px"
  nav-row-active:
    textColor: "{colors.ink}"
  topbar:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.ink}"
    padding: "14px clamp(16px, 4vw, 48px)"
    height: "70px"
  logo:
    textColor: "{colors.accent}"
    height: "26px"
  banner:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.danger}"
    typography: "{typography.small}"
    padding: "8px clamp(16px, 4vw, 48px)"
    height: "40px"
  banner-quiet:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.muted}"
  banner-action:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.small}"
    rounded: "{rounded.focus}"
  download-bar:
    backgroundColor: "{colors.track}"
    rounded: "{rounded.pill}"
    height: "6px"
    width: "160px"
  download-bar-fill:
    backgroundColor: "{colors.bar}"
    rounded: "{rounded.pill}"
  model-note:
    textColor: "{colors.muted}"
    typography: "{typography.caption}"
    padding: "0 12px"
  popover:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.ink}"
    rounded: "{rounded.box}"
    padding: "16px"
    width: "min(440px, calc(100vw - 32px))"
  drawer:
    backgroundColor: "{colors.bg}"
    width: "min(256px, 85vw)"
  scrim:
    backgroundColor: "{colors.scrim}"
  skip-link:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.pill}"
    padding: "8px 14px"
  tooltip:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.bg}"
    typography: "{typography.pill}"
    rounded: "{rounded.tip}"
    padding: "6px 10px"
    width: "min(272px, 60vw)"
  toast:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.bg}"
    typography: "{typography.small}"
    rounded: "{rounded.pill}"
    padding: "10px 16px"
  question-row:
    textColor: "{colors.ink}"
    padding: "26px 0"
  question-id:
    textColor: "{colors.ink}"
    typography: "{typography.title}"
  bar-track:
    backgroundColor: "{colors.track}"
    rounded: "{rounded.pill}"
    height: "6px"
  bar-fill:
    backgroundColor: "{colors.bar}"
    rounded: "{rounded.pill}"
    height: "6px"
  bar-fill-answer:
    backgroundColor: "{colors.accent}"
  bar-mark-answer:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.pill}"
    size: "16px"
  bar-name:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
  bar-name-other:
    textColor: "{colors.muted}"
  readout:
    textColor: "{colors.ink}"
    typography: "{typography.code-cell}"
    width: "4.5rem"
  readout-other:
    textColor: "{colors.muted}"
  fact-name:
    textColor: "{colors.muted}"
    typography: "{typography.caption}"
  fact-value:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
  callout-danger:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.ink}"
    rounded: "{rounded.box}"
    padding: "16px 18px"
  fault-loc:
    backgroundColor: "{colors.danger-tint-2}"
    textColor: "{colors.danger}"
    rounded: "{rounded.code}"
    padding: "0.1em 0.35em"
  question-fault:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.ink}"
    rounded: "{rounded.inner}"
    padding: "8px 12px"
---

# Design System: Ardana

## Overview

**Creative North Star: "The Line at Work"**

The playground is ardana.ai put to work. The landing is a sheet of white paper with one line on it that a terminal can read; the playground keeps the paper, the ink, the one gray and the terminal-gray box, and hands the developer a task instead of a command: paste a state, ask questions, press Run, read the answers, copy the request. The groundhog lockup sits at the top-left as it does on the site, the model and the presets are quiet gray links, the state and every request sit in command boxes, the questions are catalog rows between hairlines, and each answer is the one ink line among gray ones with its exact figure in mono. Run is the one ink pill.

Density is operating density, not the landing's one calm screen: 15px words, 13px labels, 12px pills, controls 28 to 40px tall (each reaching 44px under a finger), a white 256px sidebar and a 70px top row that is the landing's nav row. The world keeps the landing's three materials (ink, one gray, the pale terminal gray) and its two voices (Onest for words, Geist Mono for whatever the API reads or returns), both self-hosted. The playground's structure and behaviour came through the rebrand unchanged; only the look follows the site.

Its refusals are confirmed: the Notion workspace it replaced (warm ink, the blue accent, gray panels, hover fills) and dashboard chrome (cards, shadows, gradients). There is no hue but one red, and the red means a fault. The dark scheme is not a second theme but the logo kit's inverse, white on #0a0a0a, and it follows the system; the page offers no switch.

**Key Characteristics:**
- The landing's tokens by name and value; the playground adds only tokens derived from them (a bar track, a bar gray, a scrim) and one red for faults.
- A white 256px sidebar headed by the 26px lockup, a sticky 70px top row with the ink Run pill, one hairline beneath.
- Onest for every word; Geist Mono for code, model names, typed ids and every returned figure.
- Terminal gray only behind what the API reads or returns; a box you type in wears a 1px gray ring, a box you copy from has none.
- Questions as hairline catalog rows; the answer is the one ink line: ink bar, ink disc with a paper check, name and figure at 600.
- Flat: no shadow, no gradient, no card; separation is a 1px hairline or more space.
- Light and dark from one token set; dark is the kit's inverse and follows the system.

## Colors

The landing's achromatic palette exactly, with one red reserved for faults and a dark scheme that inverts it.

### Primary
- **Press Ink** (`accent`, #0a0a0a): the Run pill, the skip link, the answer's bar and its check disc, and the logo. It equals the ink on purpose: the decision is marked by a filled shape and by weight, not by a colour. Pure white in the dark scheme.
- **Soft Press** (`accent-hover`, #2b2b2b): Run on hover and press; its white label reads at 14.2:1.
- **On Ink** (`on-accent`, #ffffff): the Run label, the skip link's text and the check inside the answer's disc; 19.8:1 on Press Ink.

### Neutral
- **Paper** (`bg`, #ffffff): the only ground: page, sidebar, top bar, popover and drawer alike; also the text on the tooltip and the toast.
- **Ink** (`ink`, #0a0a0a): headings, body text, values, hovered and current links, the outline of the picked segment, and the fill of the tooltip and the toast.
- **Pencil Gray** (`muted`, #666666): the secondary voice: the lede, instructions, labels, sidebar links and quiet buttons at rest, placeholders, fact names, notes, and every option that is not the answer once a run has answered. 5.7:1 on Paper, 5.2:1 on Terminal Gray.
- **Hairline** (`line`, #ececec): the 1px rules under the top row and the banner, at the sidebar's right edge, above and between question rows, around the empty-list callout and above a builder panel; the outline of tags, the popover and a disabled field.
- **Bar Track** (`track`, #ececec): the empty 6px bar. It is the hairline under a name of its own, so the bar follows the hairline in both schemes.
- **Bar Gray** (`bar`, #858585): the fill of an option that is not the answer, and of an answer gone stale. The landing's Prompt Gray one step darker: the nearest gray that holds 3:1 on its track (3.1:1, where #8a8a8a gives 2.9:1).
- **Terminal Gray** (`code-bg`, #f4f4f4): the ground of every command box (the state, the questions JSON, the wire panes, the snippet, the share link's row), of the model select and the builder's fields, and of inline code.
- **Command Ink** (`code-ink`, #0a0a0a): text in command boxes and fields.
- **Prompt Gray** (`code-muted`, #8a8a8a): the 1px ring of a box you type in (3.5:1 on Paper, 3.1:1 on Terminal Gray), the curl `$` prompt, icon keys and the copy key at rest, the select's chevron, and the scrollbar thumb.
- **Icon Wash** (`code-hover`, 7% ink): the ground behind a hovered icon key or copy key, and nothing else.
- **Focus Ink** (`focus`, #0a0a0a): the 2px focus ring. Pure white in the dark scheme.
- **Selection Wash** (`selection`, 12% ink): text selection, an ink wash rather than a browser blue.
- **Ink Veil** (`scrim`, 40% ink): the scrim behind the drawer.

### Fault
- **Fault Red** (`danger`, #b42318): fault titles and icons, the fault banner's text, fault lines under fields, the ring of a refused field or editor, and the location code of a 422 issue. 6.6:1 on Paper, 5.9:1 on Fault Wash, 5.1:1 on Fault Chip.
- **Fault Wash** (`danger-tint`, #fdf0ef): the fault callout, the fault banner and a question's own fault line.
- **Fault Chip** (`danger-tint-2`, #f9dcd9): the ground of the `body › questions › id` location code inside a fault.

### Dark scheme
`prefers-color-scheme: dark` redefines the same tokens (`color-scheme: light dark`; `theme-color` #ffffff and #0a0a0a). The brand's ink (the logo, Run, the answer, the focus ring) turns pure white; text turns a soft white.

| Token | Light | Dark |
|---|---|---|
| `bg` | #ffffff | #0a0a0a |
| `ink` | #0a0a0a | #ededed |
| `muted` | #666666 | #a1a1a1 |
| `line` | #ececec | #262626 |
| `accent` | #0a0a0a | #ffffff |
| `accent-hover` | #2b2b2b | #dedede |
| `on-accent` | #ffffff | #0a0a0a |
| `code-bg` | #f4f4f4 | #171717 |
| `code-ink` | #0a0a0a | #ededed |
| `code-muted` | #8a8a8a | #7a7a7a |
| `code-hover` | rgb(10 10 10 / 0.07) | rgb(255 255 255 / 0.08) |
| `focus` | #0a0a0a | #ffffff |
| `selection` | rgb(10 10 10 / 0.12) | rgb(255 255 255 / 0.2) |
| `track` | #ececec | #262626 |
| `bar` | #858585 | #7a7a7a |
| `scrim` | rgb(10 10 10 / 0.4) | rgb(0 0 0 / 0.6) |
| `danger` | #b42318 | #ff7b72 |
| `danger-tint` | #fdf0ef | #2a1311 |
| `danger-tint-2` | #f9dcd9 | #4a1d19 |

Dark contrast: soft white text reads 16.9:1 on #0a0a0a; Pencil Gray 7.7:1 on the page and 6.9:1 on Terminal Gray; Prompt Gray 4.6:1 and 4.2:1; Bar Gray 3.5:1 on its track; Fault Red 6.9:1 on its wash and 5.6:1 on its chip; Run's near-black label 19.8:1 on white.

### Named Rules
**The One Red Rule.** The palette has no hue but one red, and the red means a fault: something the API, the JSON parser or a share link refused. No blue, no success green, no warning amber; never red for emphasis, decoration or a destructive action (Remove question is a quiet gray button).

**The Ink Is The Accent Rule.** `accent` equals ink. Run, the skip link and the answer are marked by filling a shape with ink and by weight, never by a brand colour.

**The Terminal Gray Rule.** Terminal Gray sits only behind what the API reads or returns: command boxes, fields, the model select, inline code. It is never a card, panel, sidebar or section ground, and an empty pane is a gray sentence, not an empty gray box.

**The Inverse Rule.** The dark scheme is the logo kit's inverse and nothing else: the page #0a0a0a, the brand's ink pure white, text a soft white. It follows the system; there is no switch and no third scheme.

## Typography

**Display Font:** Onest Variable (`"Onest Variable", ui-sans-serif, system-ui, sans-serif`)
**Body Font:** Onest Variable (the same stack)
**Label/Mono Font:** Geist Mono Variable (`"Geist Mono Variable", ui-monospace, "SF Mono", Menlo, monospace`)

**Character:** Onest, a warm, slightly geometric grotesk, carries every word at the landing's weights, with tight negative tracking on headings only; Geist Mono carries every string the API reads or returns and every figure. Both are self-hosted from `crates/ardana-playground/fonts/` as ardana.ai loads them (fontsource 5.3.1 and 5.3.0: one variable `wght` file per script subset under its `unicode-range`, `font-display: swap`, SIL OFL 1.1), with the two latin files preloaded; the placeholder page carries the latin subsets alone. The page's own words stay inside the latin subsets (a shortcut is spelled out, "Cmd+Enter", never ⌘↵), so a first load fetches no other file. Numerals are tabular across the page.

### Hierarchy
- **Headline, page** (700, clamp(2rem, 3.2vw, 2.75rem), 1, -0.035em): the one "Ardana playground" heading, about 41px at 1280 wide; 1.875rem below 720px. The landing's page headline at operating size.
- **Headline, notice** (700, clamp(1.75rem, 3vw, 2.25rem), 1.1, -0.03em, balanced): the placeholder page's one statement.
- **Title** (600, 1.375rem, 1.2, -0.02em): the landing's catalog row title: each question's id at the head of its row, the State and Questions headings, and the Raw exchange and Snippets toggle headings.
- **Title, compact** (600, 1.25rem, 1.2, -0.02em): below 720px, question ids together with the State, Questions, Raw exchange and Snippets headings, so an id never outsizes its section heading.
- **Lede** (400, 1.0625rem, 1.45): the one sentence under the page title, in Pencil Gray at 60ch; 0.9375rem below 720px. Also the placeholder's paragraphs.
- **Body** (400, 0.9375rem, 1.5, tabular): instructions at 65ch, option names, fact values, toggle captions, field text, empty-pane sentences; the top bar's crumb at 500.
- **Button** (600, 0.9375rem, 1.2): the Run label.
- **Link** (400, 0.9375rem, 1.2): quiet buttons and sidebar rows; 0.875rem in small buttons.
- **Small** (500, 0.875rem, 1.4): the toast and the banner; a question's fault line at 400.
- **Label** (500, 0.8125rem, 1.4): field labels, sidebar headings, builder legends, the Sent and Received captions; segments at line-height 1.
- **Caption** (400, 0.8125rem, 1.4): notes, fact names, segmented legends, the last-run line, the state's token count, fault lines, the sidebar foot.
- **Pill** (500, 0.75rem, 1.4): tags and tooltips; a score level's fit line at 400.
- **Code, block** (Geist Mono 400, 0.8125rem, 1.6): the state, the questions JSON, the wire panes and the snippet, two-space tabs. The state editor's placeholder is words, so it is set in Onest at 0.9375rem; the questions JSON's placeholder is code and stays mono.
- **Code, small** (Geist Mono 400, 0.8125rem, 1.45): the model select; typed ids and option keys in builder fields and the share link at the field's 1.5.
- **Code, cell** (Geist Mono 400, 0.875rem, tabular): every returned figure: the readout at the end of a bar row (600 for the answer) and the figures in the fact row.
- **Code, extra small** (Geist Mono 400, 0.75rem): the model name in the last-run line and the serving origin in the sidebar foot.

### Named Rules
**The Terminal Voice Rule.** Whatever is shown as the API reads or returns it is Geist Mono: the state, the questions JSON, the wire panes, the snippets, a model name, the serving origin, an id or option key in its field, and every returned figure. Whatever names or explains it is Onest, including a question's id as its heading, the option names beside the bars, and the state editor's placeholder.

**The One Heavy Title Rule.** Onest 700 appears once per page, on its h1, at -0.035em. Headings below it are 600 at -0.02em; nothing else is heavier than 600, tracked open, or set in capitals.

## Layout

The shell is the landing's nav grown into a workspace: a grid of a 256px sidebar and `minmax(0, 1fr)`, full height. The sidebar is white, sticky, scrolls on its own and ends in the one vertical hairline; its head is the left end of the 70px top row, which is the landing's nav row: the 26px lockup sits 22px from the top, as on ardana.ai, and 24px from the left, so the top row reads as one bar. Collapsing the sidebar (its own control, or Ctrl/Cmd+\\) takes the first column away at once (columns are layout, and nothing on the page animates layout) and puts the opener and the lockup at the top bar's left; the choice is remembered. Inside the sidebar: 24px between groups, 12px side padding, 28px group headings, 34px rows 2px apart, and the foot pinned to the bottom.

The main column stacks the sticky 70px top bar (at least 14px above and below, the fluid gutter at the sides, the crumb, then the actions 12px apart at the right), the banner under it (40px, only while it has something to say), then the page: centred, at most 1400px wide, 40px above, the gutter (16 to 48px) at the sides, a 20vh tail so the last block can scroll under the bar, 48px between sections. Anything the page scrolls to, by a jump or by Tab in either direction, lands 12px under the bar at its rendered height (`scroll-padding-top: calc(var(--topbar-stuck) + var(--space-3))`, the height the page measures, 70px until the bar wraps), and 12px under the banner too while a run is in flight; a text field Tab reaches scrolls whole into view when it fits. What stays on screen while the page scrolls never holds more than a third of the viewport (at 400% zoom, or with enlarged text on a small or turned screen): past that the in-flight banner's words scroll away under the bar and only its last row stays (the download bar and Stop), then the whole banner scrolls with the page, then the bar itself, the root's `banner-row`, `banner-loose` and `topbar-loose` from the measured heights; the scroll padding follows what stays. On a phone, where the columns stack, a run brings the first question (or the fault) to the top and the focus to its id (or the fault's title).

The page keeps the binding structure: the state on the left and the questions with their answers on the right, `minmax(0, 5fr) minmax(0, 7fr)` with the landing's 40px column gap, aligned to the top; then the raw exchange as two panes 24px apart, then the snippets. At 1280x800 the first viewport holds the top row, the title and lede, the state editor (`clamp(280px, 52vh, 600px)` tall) and the first question with its answer. Question rows take the landing's catalog-row padding, 26px above and below, and 14px between their parts. A bar row is the grid `16px minmax(6rem, 12rem) minmax(0, 1fr) auto` with 12px between columns at a 28px minimum (44px for a score level with its fit line), rows 4px apart (10px when score levels carry fit lines); the fact row wraps with the landing's gaps, 20px between lines and 48px between facts.

The questions column is also a size container (`container-type: inline-size`): whenever it is narrower than 30rem (a phone, or a small laptop beside the sidebar), each bar row restacks to mark, name and figure on one line with the bar beneath the name and figure, 4px by 10px apart, so the bar keeps a readable length whatever the window's width; a score level's first line is then at least 40px, room for its figure over its fit.

Spacing steps are 4, 8, 12, 16, 24, 40 and 48px, the landing's 26px row padding and the fluid gutter; 2, 6, 10, 14, 18 and 20px appear as half steps inside controls, boxes and the fact row.

Breakpoints, all `max-width`:
- **960px:** the columns stack, 40px apart, and the state editor falls to 220px; the sidebar becomes a drawer `min(256px, 85vw)` wide that slides in from the left over 200ms above the scrim, with the page inert behind it; the opener and the lockup stay in the top bar.
- **720px:** the page tightens (24px above, 40px between sections); the title falls to 1.875rem, question ids and the section and toggle headings to 1.25rem, the lede to 0.9375rem; facts form two columns (16px by 24px); builder rows wrap; the exchange panes stack; the crumb and the top bar's stale-tag slot are removed (each question carries its own tag); the lockup is 22px; Run narrows to 14px sides and fits its label, so "Running" with its caret fits the bar at 390px; the share popover is fixed under the top bar with 16px margins.
- **389px:** the lockup is 18px (above the kit's 80px minimum width), the top bar's gaps tighten, and Share becomes a 32px key showing the link glyph, its word visually hidden and its name still "Share link", so the bar keeps Share and Run on its one line.

## Elevation & Depth

The system is flat, the landing's Flat Paper exactly: no shadow, no gradient, no layered surface. Structure is whitespace, 1px hairlines and the single tonal step of Terminal Gray. Layers that sit over the page are told apart without depth: the tooltip and the toast by inversion (an ink card on paper), the share popover by a 1px hairline, the drawer by the Ink Veil (40% ink; 60% black in the dark scheme). They stack by order alone: the top bar at 20, tooltips and the popover at 30, the toast at 40, the scrim at 55, the drawer at 60, the skip link at 70.

### Shadow Vocabulary
Nothing casts a shadow. `box-shadow` draws only 1px inset lines, so an edge never changes a box's size:
- **Divider** (`box-shadow: inset -1px 0 0 var(--line)` / `inset 0 -1px 0 var(--line)`): the sidebar's right edge; the bottom edge of the sidebar head, the top bar and the banner.
- **Type ring** (`box-shadow: inset 0 0 0 1px var(--code-muted)`): a box you type in: fields, the model select, the state and questions editors.
- **Fault ring** (`box-shadow: inset 0 0 0 1px var(--danger)`): a field or editor whose value was refused (`aria-invalid`).
- **Rest ring** (`box-shadow: inset 0 0 0 1px var(--line)`): a field the builder cannot edit, drawn as an outline on paper.
- **Picked ring** (`box-shadow: inset 0 0 0 1px var(--ink)`): the checked segment.

### Named Rules
**The Flat Paper Rule.** Nothing casts a shadow. A layer that floats is set apart by inversion, a hairline or a scrim; a block on the page by a hairline or more space.

## Shapes

The landing's three shapes, at work: full pills (999px) for what you press hard or what marks (Run, the skip link, tags, segments, the toast, the bars and the answer's disc); softly rounded 10px boxes for what holds text the API reads (command boxes, fields, the select) and for the popover and the fault callout; and the concentric 6px inner corner for keys and rows (quiet and icon buttons, the copy key, sidebar rows, a question's fault line, the tooltip's painted card). Inline code takes 4px; a focus ring on a target with no shape of its own (a disclosure's summary, a defined term) follows a 3px corner. Everything else is square.

The icons are drawn as the landing draws them: a 16-unit grid, one 1.5px stroke with round caps and joins, in the current colour, at 16px (14px in the select); the answer's check is 11px at a 2.2 stroke inside its 16px disc. The copy, check and arrow glyphs are the landing's own, and the arrow keeps the landing's 1.6 stroke. The sidebar's opener and closer draw a window with its sidebar (the panel glyph); on the smallest phones a link glyph stands in for Share's word. A disclosure opens on a chevron that turns 90°; the landing's arrow is kept for navigation (Restore previous leads with it, mirrored; the banner's "Show the command" ends with it). No icon is a text glyph.

The logo is the kit's tight horizontal lockup (`crates/ardana-playground/brand/ardana-logo.svg`), inline in `currentColor` from `accent`: ink on paper, white on #0a0a0a, 26px tall (22px in a phone's top bar, 18px at 389px and below, above the kit's 80px minimum width). Its drawing is in the page once, as a `<symbol>` that the sidebar's logo and the top bar's both use. The height is set on its outer svg only, so the nested mark keeps its own geometry; it is never recoloured, stretched or outlined. The favicons are the kit's; the SVG one follows the scheme.

### Named Rules
**The Three Shapes Rule.** A pill, a 10px box or its 6px inner key; 4px belongs to inline code and 3px to focus on shapeless text. A new component picks one of the three.

**The Hairline Rule.** Rules are 1px. Between regions and rows they run horizontal; the sidebar's right edge is the one vertical rule, starting under the top row. Tags, the popover and a disabled field are the only closed outlines.

## Components

Quiet at rest, certain when pressed: words are gray until the pointer or the page makes them current, then ink, with no fill; the one filled thing is Run. Colours, rings and washes change over 150ms; the drawer, the scrim, the skip link, the disclosure chevron and the back arrow move over 200ms on the landing's ease-out (`cubic-bezier(0.16, 1, 0.3, 1)`). Whatever moves, moves by `transform` or `opacity` alone, never by a layout property. Under `prefers-reduced-motion` every transition and the bar growth stop and the caret holds steady; under `forced-colors` controls draw a 1px system border, Run and the answer's bar and disc take `Highlight`, and focus is `Highlight`.

### Buttons
- **Shape:** quiet and icon buttons take the 6px inner corner; Run is a full pill.
- **Quiet (default):** transparent, Pencil Gray at 15px and 400, 32px tall with 10px sides; hover, press and `aria-expanded="true"` turn the label ink, with no fill. Share and Edit are quiet; Share is its word, and only at 389px and below a 32px key showing the link glyph, the word visually hidden and the name still "Share link". Small (Edit, Add option, Remove question): 28px tall, 8px sides, 14px. A held button carries `aria-disabled="true"`, never `disabled`, sits at 45% opacity and keeps its focus.
- **Primary (Run):** Press Ink with a white 15px label at 600, 36px tall, 18px sides; hover Soft Press; a press scales it to 0.97. Held at 35% opacity with its reason in the banner (`aria-describedby`, the banner's sentence alone, never its bar or its actions): the questions JSON does not parse, the picked model is one this server does not run (the ardana CLI pulls it onto a local server, or runs it on the visitor's machine; Run never pulls), or there is no question; busy at full strength, the label reading Running (Pulling while the server pulls a browser model's files, Loading while this tab downloads or starts a browser model) with the landing's blinking block caret after it. Held or busy, it shows no tooltip: "Send the state and questions" would be untrue then, and at the pill's faded strength it would cover the banner. It keeps one width, as the landing's Copy pill does when it says Copied: its face and an unseen "Running" with its caret share one grid cell, so above 720px the pill is as wide as its widest face at rest and in flight; in a phone's top bar the unseen face is dropped, the sides narrow to 14px and the pill fits its label. Ctrl/Cmd+Enter presses it from anywhere, and its tooltip says so.
- **Icon:** a 32px key (28px small) in Prompt Gray that turns Command Ink on the Icon Wash when hovered; its name is its `aria-label`, repeated in its tooltip. The sidebar's opener and closer, both the panel glyph (the closer hidden until the sidebar head is hovered or the sidebar holds focus), Remove option and Remove level.
- **Focus:** the landing's ring on every control: 2px Focus Ink, 3px out, following the control's corner.
- **Touch:** under a coarse pointer (`pointer: coarse`) every control reaches at least 44px by 44px (WCAG 2.5.5) and looks as it does under a mouse: a key, a segment, a toggle's heading or a banner action reaches past its drawn edges through a transparent box around it (`::before`, inset to 44px), while sidebar rows, fields, the select, the share link's box and the banner, which have no box to lend, are 44px tall; wrapped segments keep 16px between rows. A fine pointer keeps the sizes above.

**The Touch Rule.** A finger never meets a target under 44px, and a mouse never meets a control larger than it is drawn.

**The One Ink Pill Rule.** Run is the only filled button on a screen, and above 720px it keeps one width while it works. The skip link is an ink pill too, but it appears only on focus.

A run answered in this tab reads like a server's: the last-run line names where ("Answered by decider-0.8b-v1 in this tab on WebGPU"), the raw exchange's Sent caption reads "Sent · in this tab" (from the pick, before a first run), and the figures, the fault callout and the raw body are drawn as for the API, because they are the API's bytes. A run in this tab that gets no answer at all (its connection dropped, the server could not provide the files, the runtime did not start) says so in its own words: the fault callout's title is "Not answered in this tab." and its one line names the cause and the next step ("The download of decider-0.8b stopped at 20.0 MB of 467.7 MB. Run again once the connection is back to resume it.", or "… to start it over: a page without HTTPS keeps no files." where the page keeps nothing; "Reload the page, then run again." only where running again cannot work), while the raw exchange keeps the reason as the browser gave it. Before a first run with a model the server has not pulled, the caption is plain "Sent" over "Nothing sent yet.", since Run sends nothing.

### Chips
- **Tag:** the landing's "default" pill: transparent, a 1px Hairline outline, Pencil Gray at 12px and 500, 2px by 8px. "Sent as text" or "Sent as JSON" in the state's head, "Inputs changed" in the top bar, "From last run · inputs changed" on a stale question. Static, never interactive. Its words wrap only when wider than their row (enlarged text on a phone); the top bar's tag gives way with an ellipsis instead.
- **Segmented control:** a native radio group after a 13px Pencil Gray legend at 400, 12px before its options, so the legend never reads as one more option. Segments are 28px pills with 12px sides, 13px at 500 in Pencil Gray (ink on hover); the checked one reads in ink inside a 1px ink ring, so the pick never rests on colour alone. The real radio covers its segment invisibly, so the arrow keys move the pick; focus rings the segment. With enlarged text the group wraps, its options under the legend and onto further rows 4px apart, rather than run off a phone's screen. Used for a question's type (noul, choice, score) and the snippet's language.

### Command boxes (signature, from the landing)
The landing's core object at work: a Terminal Gray box with 10px corners, Geist Mono 13px at 1.6, 16px by 18px of padding. The snippet keeps the landing's 40px copy key in its top-right corner and the gray `$` prompt before a curl or ardana line (drawn, never copied or read out). The copy key is Prompt Gray at rest and Command Ink on the Icon Wash when hovered; after a copy its glyph fades and shrinks as a check scales in, for 1.6s. A copy that worked is said to screen readers only; one the browser refused is said in the toast. An empty wire pane is a 15px Pencil Gray sentence. Bodies wrap; they never truncate.

### Inputs / Fields
- **Style:** a 36px Terminal Gray box with 10px corners, 6px by 12px of padding, Command Ink at 15px (typed ids and option keys in Geist Mono 13px), placeholders in Pencil Gray, under a 13px Pencil Gray label at 500, 6px above; a 13px note or fault line beneath.
- **Focus:** the 2px Focus Ink ring, 3px out.
- **Error:** `aria-invalid="true"` turns the ring Fault Red, and the fault line beneath reads the refusal in Fault Red.
- **Disabled:** a field the builder cannot edit (structured JSON) is a Hairline outline on Paper with gray text, and its note says where to edit it.
- **Model select:** the landing's small command box as a real `select`: 40px, 10px corners, the type ring, the model's name in Geist Mono 13px, a 14px chevron in Prompt Gray at the right that turns Command Ink on hover. Pulled models first, then the models that run in this tab ("In browser · runs in this tab", each with the bytes its first run downloads), then the library models this server has not pulled ("Library · runs with the ardana CLI", each with its download size); a model both pulled and browser-capable has a row in each, and each runs where it says. The page opens on the default model when the server has pulled it, else on the browser default's row in the tab (a server with nothing pulled, the standalone build). The list is read again after each run and whenever the tab comes back, so a model `ardana pull` added meanwhile moves to Pulled and Run sends it.
- **Model note:** under the select, while the pick does not run on the server, one 13px Pencil Gray caption at the heading's 12px inset, 6px below the box, read as the select's description: "Runs in this tab. The first run downloads 467.7 MB, which this browser keeps. Running it takes 3 to 4 times that in memory; only a reload frees all of it." for an "In browser" row ("… The first run on each visit downloads 467.7 MB: a page without HTTPS keeps no files. …" where the page is no secure context and has nowhere to keep them; 3 to 4 times holds for every browser model on WebGPU and on WASM, and picking a server row frees only part of it), "Not pulled on this server. Pull it with the ardana CLI where the server runs (2.7 GB), then Run answers here." for a model a local server has not pulled (without the size for a quant the list does not name), and in the standalone build, which no server serves, "Runs with the ardana CLI on your machine; its first run downloads 2.7 GB." It names where Run answers before anything downloads.
- **Share link:** read only, a thing to copy: the input sits bare in a 40px Terminal Gray row with the copy key at its end, the landing's small command box with no ring; focus rings the whole row.

- **The ardana commands:** where this server does not run the pick (an "In browser" row, or a model it has not pulled), Snippets hands over the CLI instead of the language control: a 15px Pencil Gray line, then one or two steps 24px apart, each a 13px label over its box with a 13px note under it, the last "… run the request" over the snippet box holding `ardana run <name> --request -` with the exact request in a heredoc. On a local server, which ardana runs already, a model it has not pulled comes first as `ardana pull <name>` in the landing's small command row (40px, its copy key at the end): for its server row, "This server runs decider-4b once the ardana CLI pulls it.", "Pull it where the server runs" with "Downloads 2.7 GB; then Run answers here." under it, then "Or run the request in a terminal"; for an "In browser" row, "The ardana CLI runs this request in a terminal.", "Pull decider-0.8b first" with what it downloads, then "Then run the request", or "Run the request" alone once the server has pulled the model. In the standalone build, which no server serves, ardana.ai's install line takes the pull's place: "decider-4b runs with the ardana CLI on your machine." for a server row ("The ardana CLI runs this request on your own machine." for an "In browser" row), "Install ardana" over `$ curl -fsSL https://ardana.ai/install.sh | sh` (the Windows line in inline code beneath), then "Then run the request" and what its first run downloads.

**The Ring Rule.** A box you type in (a field, the select, the state and questions editors) wears the 1px Prompt Gray ring; a box you copy from (the wire panes, the snippet, the share link's row) wears none. The ring says "type here"; the bare box says "copy this".

### Navigation
- **Sidebar:** white, 256px, the one vertical hairline at its right. The head is the 26px lockup, 22px from the top; then Model (the select), Presets and "On this page" under 13px Pencil Gray headings; the foot holds the serving origin in Geist Mono 12px and the Run shortcut, in Pencil Gray.
- **Rows:** a preset or a section, set as the landing sets a nav link: 15px at 400 in Pencil Gray, 34px tall with 12px sides; ink on hover, with no fill and no change of weight. The current row (`aria-current`, the section whose top has passed under the bar) is ink at 500: the weight tells it apart where the ink alone does not (2.2:1 against the gray in the dark scheme; forced colours draw every row alike). "Restore previous" leads with the landing's arrow mirrored, which nudges 3px back on hover; it brings back the editors a preset, a removed question or a share link replaced, and the row a share link picked over.
- **Top bar:** the landing's nav row: Paper, 70px, one hairline beneath; the crumb "Playground" in 15px ink at 500, then the stale tag, Share and Run at the right. Share and Run never shrink; the tag gives way first, then the crumb. Share's popover opens 8px under the hairline, never across it, and closes on a click outside, when the focus moves on beyond it, and on Escape wherever the focus is, which then returns to Share. Taller than a third of the viewport (400% zoom with enlarged text), the bar scrolls with the page. The document title follows the site's "Page · Ardana" pattern: "Playground · Ardana".
- **Drawer (below 960px):** the sidebar fixed at the left, `min(256px, 85vw)`, sliding in over 200ms above the Ink Veil; the page behind is inert; Escape or the scrim closes it, and a pick inside closes it and moves focus on.
- **Skip link:** "Skip to the page", the landing's ink pill sliding in from above the top-left corner on focus (8px by 14px, 14px text). It scrolls to the page and gives it the focus without touching the URL or the history (Back never reloads a share link over the editors), and it is inert under the open drawer, as the page is.

### Question row (signature)
A catalog row, not a card: a hairline above the list and under each row, the landing's 26px of air, no fill and no frame. The head sets the id, at the landing's row-title size (the compact Title below 720px, with the section headings), beside the type control and Edit; the instructions follow in 15px Pencil Gray at 65ch. Edit opens the builder under a hairline inside the row: the id and instructions fields, then option or level rows with a legend counting them against their bounds, Add held at the upper bound and Remove at the lower, and Remove question at the right.
- **Bar row:** a 16px mark, the option's name, a 6px Bar Track pill whose fill grows to the returned probability over 600ms on the ease-out (the fill, as long as its track, slides in from the left by `transform`, its rounded end leading, and the track clips it to its own pill), and the exact figure right-aligned in Geist Mono 14px in a 4.5rem column. In a questions column narrower than 30rem the bar drops beneath the name and figure. Before a run the options sit in ink on empty tracks with a gray "–".
- **The answer:** once a run has answered, the ink disc with its paper check, the name and figure at 600, the bar in Press Ink. A noul row is a single probability, not a pick: no mark, its bar ink and its figure at 600 like a picked answer's.
- **Fit:** a score level's fit line ("fit" and a percent, 12px) sits in its own figure's cell, under the figure, in a row at least 44px tall, so it can only belong to its level.
- **Stale:** when the inputs no longer match the run, the figures stay but the answer's ink goes Bar Gray, the readouts go Pencil Gray, and a tag says so.
- **Fact row:** under the bars, the landing's fact row: 13px Pencil Gray names over 15px values, 48px between facts and 20px between lines, wrapping (two columns below 720px). The Answer is the option's name at 600; every other fact is a figure in Geist Mono 14px. Each name is a defined term whose meaning shows in a tooltip on hover or focus, opening away from the nearer page edge so it is never cut: above 960px every fact after the first opens leftwards; from 721 to 960px, where the questions column starts at the page's edge, facts open rightwards and the fifth and later leftwards; at 720px and below, in two columns, the left column's open rightwards and the right column's leftwards. While a run is in flight they open downwards, clear of the banner under the top bar. A fact row is one stop for Tab, its first name, whose tooltip then adds "Left and Right arrows: the other facts" on a second line; Left and Right (and Home and End) move along the row. A name breaks, hyphenated, only when enlarged text makes it wider than its column.
- **Fault:** a 422 issue that names the question appears under its head as a 14px line on Fault Wash with a Fault Red alert glyph, 6px corners.
- **Add question:** an ink row at 15px and 500, led by the plus glyph, under the list, as the landing's forward links read.

**The One Ink Line Rule.** Once a run has answered, each question has one ink line: its answer's bar, disc, name and figure. Every other option's name, bar and figure speaks in the gray. A noul's probability is the whole answer, so its bar is ink and its figure 600; a stale answer keeps its figures and gives up its ink.

**The Catalog Row Rule.** Questions are rows between hairlines, never cards: no fill, no frame, no radius around a question.

### Callouts and the banner
- **Empty list:** a row of its own between two hairlines, Pencil Gray text at 60ch after the info glyph, with inline code for `noul`, `choice` and `score`.
- **Fault (the one red box):** Fault Wash with 10px corners and 16px by 18px of padding; the alert glyph and the title ("HTTP 422 · the request was not answered.", or "Not answered in this tab." for a run in the tab that got no answer) in Fault Red at 600; each issue as its location code (Fault Red on Fault Chip, 4px corners, Geist Mono) followed by its message in ink. It stands at the top of the questions column after a refused run.
- **Banner:** the line under the top bar: 40px, 14px at 500, a hairline beneath, hidden when empty. Quiet (Paper, Pencil Gray) for a wait or a hint: where a run in this tab is (the server pulling its browser files, said only when its model list marks them not held, the tab downloading them, starting the model, running it on WebGPU or WASM), why Run is held, or, before a run in this tab that will download, what it downloads and then takes in memory: "Run downloads decider-0.8b into this tab once: 467.7 MB, kept by this browser. Running it takes 3 to 4 times that in memory." ("… on each visit: 467.7 MB, as a page without HTTPS keeps no files. …" where the page cannot keep them), so the cost is said where Run is, at every width, and an autorun link waits for the tap. When Run is held because the server does not run the model ("This server has not pulled decider-4b: pull it with the ardana CLI" on a local server; "decider-4b runs with the ardana CLI on your machine" in the standalone build), the line ends in "Show the command", set as the landing's forward link (ink, the arrow after it, nudging 3px on hover): it opens Snippets under the bar and focuses it. In the standalone build a second action follows 24px on, "Run decider-0.8b in this tab instead" (the browser default), the ink link without the arrow, as Stop: it picks that model's "In browser" row and gives Run the focus, whose description then says what its first run downloads. Where the words and the actions do not fit one line, the actions take the next line together. While the server pulls or the tab downloads, the line ends in "Stop", the same ink link without the arrow (underlined on hover, as the landing underlines its ink links): the download ends where it is, Run says Run again with the focus on it, nothing is answered, and the status says "Stopped, not answered. The next run resumes the download." (the next run asks for the rest by byte range; a page without HTTPS starts it over); picking another row stops it too. While a run in this tab is in flight the banner stays right under the top bar (sticky, at the bar's rendered height), its only progress when the page is scrolled, and whatever takes the focus scrolls clear of both, however many lines the bar and the banner take at that width and text size (WCAG 2.4.11). Together they hold at most a third of the viewport: past that only the banner's last row stays, the download bar and Stop, its words scrolling away under the bar, and past that the banner scrolls with the page, Stop always the next stop after Run. Stop leaving once the download is over gives its focus to Run. Under a coarse pointer the banner is 44px tall. Fault colours (Fault Red on Fault Wash) only when the questions JSON does not parse.
- **Download bar:** while this tab downloads a browser model, the banner's words count the bytes it has of the bytes it needs, one count across its three files ("Downloading decider-0.8b into this tab: 118.4 MB of 467.7 MB") and a 6px pill bar 8px after them shows the same: the answers' Bar Track with a Bar Gray fill, 160px wide (96px at least), never ink, since nothing has been answered yet. At 720px and below the words take their lines and the bar fills the next row, Stop at its end, at any text size. It is a native `progress`, redrawn every half percent, with no transition, hidden from assistive technology (the words carry the count, and the status region says it in tenths); forced colours draw it in CanvasText with a Highlight fill.

### Tooltip and toast
- **Tooltip:** an ink card (soft white in the dark scheme) with Paper text, Onest 12px at 500, centred, 6px by 10px, at most `min(272px, 60vw)`, drawn from `data-tip` above, below, at the start or at the end of its control; a second line names a shortcut ("Ctrl+Enter or Cmd+Enter", spelled out). It shows on focus, on hover where a pointer can hover, and stays while the pointer crosses onto it: a 6px transparent border bridges the gap, so its 12px outer corner paints at the 6px inner one. Escape dismisses every tooltip. It describes; it never names a control alone, and it is drawn, never read out (`content: attr(data-tip) / ""`), so it never joins a control's accessible name. A held or busy Run shows none.
- **Toast:** an ink pill fixed 16px from the bottom right, 10px by 16px, 14px at 500, only for what the clipboard refused.

### Disclosure blocks
Native `details`, open by default, for the raw exchange and the snippets: a 20px chevron in Pencil Gray (ink on hover) that turns 90° over 200ms when open, the heading at Title size, and a 15px Pencil Gray caption ("Last run, byte for byte", "The request as it stands"). The content sits 16px below.

### Caret
The landing's one ornament: a block caret (0.55em by 1em, in the current colour) blinking every 1.1s in steps, only after Run's label while a run is in flight; steady under reduced motion.

## Do's and Don'ts

### Do:
- **Do** read every colour from the `base.css` tokens, which keep the landing's names and values; derive a new token from a landing token (as `track`, `bar` and `scrim` are) and redefine it under `prefers-color-scheme: dark`.
- **Do** set whatever the API reads or returns in Geist Mono on Terminal Gray, ring a box you type in with 1px Prompt Gray, and leave a box you copy from bare.
- **Do** mark the answer with ink alone: the ink bar, the ink disc with its paper check, the name and figure at 600, every other option in the gray.
- **Do** keep a stale answer's figures and take away only its ink.
- **Do** answer hover by turning gray words ink; keep the Icon Wash for icon keys and the copy key.
- **Do** keep the landing's focus ring (2px Focus Ink, 3px out) on every control, and stop every transition under reduced motion while the caret holds steady.
- **Do** self-host Onest and Geist Mono from `fonts/` under fontsource's `unicode-range`s, and preload the two latin files.
- **Do** draw the logo from the kit only, in `currentColor` from `accent`, 26px tall, white on #0a0a0a in the dark scheme.
- **Do** keep the state on the left and the questions with their answers on the right; a restyle changes the look, never the structure or the behaviour.

### Don't:
- **Don't** introduce a hue: no blue, no success green, no warning amber; red belongs to faults only, never to emphasis, decoration or a destructive action.
- **Don't** cast a shadow, draw a gradient or put content in a card; `box-shadow` draws only 1px inset lines.
- **Don't** use Terminal Gray as a card, panel, sidebar or section ground, or behind an empty pane.
- **Don't** set words in Geist Mono, or code and figures in Onest.
- **Don't** load a font, icon, image or script from another origin.
- **Don't** add a second filled button to a screen; Run is the one ink pill.
- **Don't** add ornament beyond the caret, which appears only while a run is in flight.
- **Don't** open a disclosure on the landing's arrow or draw an icon with a text glyph; the arrow is for navigation, the chevron for disclosure.
- **Don't** add a theme switch or a third scheme; the page follows the system.
