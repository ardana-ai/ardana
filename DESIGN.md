---
name: Ardana
description: A Notion workspace page for running a state and questions against a local System 1 decision model, where calibrated answers read as property bars.
colors:
  page: "#ffffff"
  panel: "#fbfbfa"
  raised: "#ffffff"
  code: "#f7f6f3"
  ink: "#37352f"
  ink-2: "rgb(25 23 17 / 0.65)"
  ink-3: "rgb(55 53 47 / 0.45)"
  hairline: "#e9e9e7"
  hairline-strong: "rgb(55 53 47 / 0.16)"
  edge-strong: "rgb(55 53 47 / 0.55)"
  hover: "rgb(55 53 47 / 0.08)"
  pressed: "rgb(55 53 47 / 0.16)"
  field: "rgb(242 241 238 / 0.6)"
  tag: "rgb(227 226 224 / 0.5)"
  tag-ink: "#32302c"
  track: "rgb(55 53 47 / 0.14)"
  bar-muted: "#787774"
  accent: "#2383e2"
  accent-fill: "#0075d3"
  accent-fill-hover: "#0066bd"
  accent-tint: "rgb(35 131 226 / 0.14)"
  on-accent: "#ffffff"
  selection: "rgb(35 131 226 / 0.3)"
  danger: "#b3372f"
  danger-tint: "#fdebec"
  danger-tint-2: "rgb(212 76 71 / 0.16)"
  tooltip: "#0f0f0f"
  tooltip-ink: "rgb(255 255 255 / 0.9)"
  overlay: "rgb(15 15 15 / 0.6)"
  scroll-thumb: "#d3d1cb"
typography:
  page-title:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "2rem"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  page-title-compact:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "1.5rem"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  block-heading:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "normal"
  ui:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: "normal"
  body:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
    letterSpacing: "normal"
    fontFeature: "tnum"
  small:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "0.8125rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "normal"
  label:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "normal"
  caption:
    fontFamily: 'ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, "Apple Color Emoji", Arial, sans-serif, "Segoe UI Emoji", "Segoe UI Symbol"'
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.4
    letterSpacing: "normal"
  code:
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", Courier, monospace'
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "normal"
rounded:
  xs: "3px"
  sm: "4px"
  md: "6px"
  lg: "8px"
  tooltip: "10px"
spacing:
  space-1: "4px"
  space-2: "8px"
  space-3: "12px"
  space-4: "16px"
  space-5: "24px"
  space-6: "32px"
  space-8: "48px"
components:
  button:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.ui}"
    rounded: "{rounded.md}"
    padding: "0 8px"
    height: "28px"
  button-hover:
    backgroundColor: "{colors.hover}"
  button-active:
    backgroundColor: "{colors.pressed}"
  button-disabled:
    textColor: "{colors.ink-3}"
  button-primary:
    backgroundColor: "{colors.accent-fill}"
    textColor: "{colors.on-accent}"
    typography: "{typography.ui}"
    rounded: "{rounded.md}"
    padding: "0 12px"
    height: "28px"
  button-primary-hover:
    backgroundColor: "{colors.accent-fill-hover}"
  button-primary-disabled:
    backgroundColor: "{colors.accent-fill}"
    textColor: "{colors.on-accent}"
  button-icon:
    backgroundColor: "transparent"
    textColor: "{colors.ink-2}"
    rounded: "{rounded.md}"
    padding: "0"
    width: "28px"
    height: "28px"
  button-sm:
    typography: "{typography.small}"
    height: "24px"
  segmented-control:
    backgroundColor: "{colors.hover}"
    rounded: "{rounded.md}"
    padding: "2px"
  segment:
    backgroundColor: "transparent"
    textColor: "{colors.ink-2}"
    typography: "{typography.small}"
    rounded: "{rounded.sm}"
    padding: "0 10px"
    height: "24px"
  segment-on:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
  field:
    backgroundColor: "{colors.field}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "3px 8px"
    height: "28px"
  field-label:
    textColor: "{colors.ink-2}"
    typography: "{typography.label}"
  field-disabled:
    backgroundColor: "transparent"
    textColor: "{colors.ink-2}"
  select:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "3px 28px 3px 8px"
    height: "28px"
  select-hover:
    backgroundColor: "{colors.hover}"
  tag:
    backgroundColor: "{colors.tag}"
    textColor: "{colors.tag-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.xs}"
    padding: "0 6px"
    height: "20px"
  tooltip:
    backgroundColor: "{colors.tooltip}"
    textColor: "{colors.tooltip-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.tooltip}"
    padding: "4px 8px"
    width: "min(272px, 60vw)"
  toast:
    backgroundColor: "{colors.tooltip}"
    textColor: "{colors.tooltip-ink}"
    typography: "{typography.small}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
  banner:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.danger}"
    typography: "{typography.small}"
    padding: "6px 16px"
    height: "36px"
  banner-quiet:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
  sidebar:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    padding: "8px 8px 16px"
    width: "240px"
  sidebar-heading:
    textColor: "{colors.ink-2}"
    typography: "{typography.label}"
    padding: "0 8px"
    height: "24px"
  sidebar-row:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.ui}"
    rounded: "{rounded.sm}"
    padding: "0 8px"
    height: "28px"
  sidebar-row-hover:
    backgroundColor: "{colors.hover}"
  sidebar-row-active:
    backgroundColor: "{colors.pressed}"
  sidebar-row-current:
    backgroundColor: "{colors.hover}"
    textColor: "{colors.ink}"
  workspace-mark:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.page}"
    rounded: "{rounded.sm}"
    size: "20px"
  topbar:
    backgroundColor: "{colors.page}"
    textColor: "{colors.ink}"
    typography: "{typography.ui}"
    padding: "0 12px"
    height: "45px"
  question-block:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "16px"
  question-fault:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.ink}"
    typography: "{typography.small}"
    rounded: "{rounded.sm}"
    padding: "8px 12px"
  bar-row:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    height: "24px"
  bar-mark:
    backgroundColor: "transparent"
    rounded: "{rounded.xs}"
    size: "16px"
  bar-mark-winner:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
  bar-track:
    backgroundColor: "{colors.track}"
    rounded: "{rounded.xs}"
    height: "6px"
  bar-fill:
    backgroundColor: "{colors.bar-muted}"
    rounded: "{rounded.xs}"
    height: "6px"
  bar-fill-winner:
    backgroundColor: "{colors.accent}"
  property-row:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    height: "28px"
  property-name:
    textColor: "{colors.ink-2}"
    typography: "{typography.body}"
  callout:
    backgroundColor: "{colors.code}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "16px 16px 16px 12px"
  callout-danger:
    backgroundColor: "{colors.danger-tint}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "16px 16px 16px 12px"
  fault-loc:
    backgroundColor: "{colors.danger-tint-2}"
    textColor: "{colors.danger}"
    rounded: "{rounded.xs}"
    padding: "0.1em 0.3em"
  toggle-block:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.block-heading}"
    rounded: "{rounded.sm}"
    padding: "0 6px 0 0"
  toggle-block-hover:
    backgroundColor: "{colors.hover}"
  toggle-marker:
    textColor: "{colors.ink-2}"
    size: "24px"
  code-block:
    backgroundColor: "{colors.code}"
    textColor: "{colors.ink}"
    typography: "{typography.code}"
    rounded: "{rounded.lg}"
    padding: "16px"
  inline-code:
    backgroundColor: "{colors.hover}"
    rounded: "{rounded.xs}"
    padding: "0.1em 0.3em"
  popover:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "12px"
    width: "min(420px, calc(100vw - 24px))"
  drawer:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    padding: "8px 8px 16px"
    width: "min(240px, 85vw)"
  scrim:
    backgroundColor: "{colors.overlay}"
  skip-link:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
    typography: "{typography.ui}"
    rounded: "{rounded.md}"
    padding: "6px 10px"
---

# Design System: Ardana

## Overview

**Creative North Star: "The Workspace Page"**

The playground is a page in a workspace the developer already knows: a paper-grey sidebar of settings and pages on the left, one white page of blocks on the right, a thin sticky bar above it holding the page's name and its one blue button. Notion's app grammar is played straight, with Linear and GitHub as the craft bar: the system sans at 14px, warm ink on white, hairlines where other products draw borders, small radii, chrome that shows itself on hover, toggle blocks, callouts and property rows. The work is the blocks. The state is a code block, each question is a white block with a hairline, and every returned probability is a property with a low progress bar and its exact figure at the end of the row.

Density is product-UI density: 28px controls, 24px rows inside blocks, 12px between blocks, 32px between the sections of the page. Nothing is decorated. There is no hardware or instrument metaphor, no engraved legend, no glowing readout, no web font, no gradient. Elevation is a 1px hairline shadow or a divider; only surfaces that float (the share popover, the drawer, the toast) cast a real shadow. The page follows the system scheme and offers no switch of its own: the same tokens turn a white page into a `#191919` one.

Colour is spent on meaning. One blue marks the decision: the Run button, focus, text selection, and the winning row of an answer. Red marks a fault: the callout for a refused request, the fault banner and fault lines, an invalid field's hairline, and the one action that destroys a question. Everything else is ink at three strengths on white, paper-grey and code-grey.

**Key Characteristics:**
- A 240px workspace sidebar beside one page of blocks, under a sticky 45px top bar with the blue Run.
- The system sans and system mono only; 14px UI, 13px code, 16px block headings, one 32px page title.
- Warm ink `#37352f` on white, `#fbfbfa` sidebar, `#e9e9e7` hairlines, hover fills at 8% ink.
- Radii of 3, 4, 6 and 8px; a 10px tooltip is the roundest thing on the page.
- Answers as property rows: a 16px check, the option name, a 6px progress bar, the exact figure.
- Blue for the decision, red for the fault, grey for everything else; light and dark from one token set.

## Colors

A warm, near-neutral palette with one blue and one red, each with a tint, drawn so that the same names carry a dark scheme.

### Primary
- **Notion Blue** (`accent`, #2383e2): the winning row's bar and check, the focus outline on every control, the inset ring of a focused field. Not a text colour on white (3.9:1); it marks and outlines.
- **Run Blue** (`accent-fill`, #0075d3): the fill of the one primary button. White text on it reads at 4.68:1; the hover fill (`accent-fill-hover`, #0066bd) at 5.78:1.
- **Blue Tint** (`accent-tint`, 14% blue): the 2px halo outside a focused field's blue ring.
- **Selection** (`selection`, 30% blue): `::selection` across the page.
- **On Accent** (`on-accent`, #ffffff): text and the check glyph on a blue fill.

### Neutral
- **Page** (`page`, #ffffff): the page and the top bar.
- **Panel** (`panel`, #fbfbfa): the sidebar, the drawer, and the quiet banner.
- **Raised** (`raised`, #ffffff): a block that sits on the page: question blocks, the popover, the checked segment, the skip link.
- **Code** (`code`, #f7f6f3): code blocks, the empty wire panes, and the plain callout.
- **Ink** (`ink`, #37352f): body text, headings, values; 12.3:1 on white.
- **Ink 2** (`ink-2`, 65% of a near-black): captions, property names, placeholders, instructions, idle readouts and inactive icons; about 5.5:1 over white.
- **Ink 3** (`ink-3`, 45% ink): disabled button labels and the glyphs in property names only; below text contrast by design, never for live text.
- **Hairline** (`hairline`, #e9e9e7): the divider under the top bar, beside the sidebar, above a property list and a builder panel.
- **Hairline Strong** (`hairline-strong`, 16% ink): the inset outline of fields, the select and an unchecked check box.
- **Edge Strong** (`edge-strong`, 55% ink; 45% white in dark): the ring of the checked segment, 3:1 on its track in both schemes.
- **Hover** (`hover`, 8% ink): the hover fill of buttons, rows and toggle summaries, the segmented control's track, inline code.
- **Pressed** (`pressed`, 16% ink): the active fill of a button or row, and an expanded button (Edit while its builder is open).
- **Field** (`field`, 60% of #f2f1ee): the fill of an input.
- **Tag** (`tag`, 50% of #e3e2e0) with **Tag Ink** (`tag-ink`, #32302c): the grey chips "Sent as text", "Inputs changed", "From last run · inputs changed".
- **Track** (`track`, 14% ink) and **Bar Muted** (`bar-muted`, #787774): the empty bar and the fill of every row that did not win, and of every winner whose answer has gone stale.
- **Tooltip** (`tooltip`, #0f0f0f) with **Tooltip Ink** (`tooltip-ink`, 90% white): the dark tooltip card and the toast.
- **Overlay** (`overlay`, 60% of #0f0f0f): the scrim behind the drawer.
- **Scroll Thumb** (`scroll-thumb`, #d3d1cb): thin scrollbars everywhere.

### Fault
- **Danger** (`danger`, #b3372f): fault titles and icons, the fault banner's text, fault lines under fields, the invalid field's hairline, the location code in a 422 issue, and the Remove question button. 5.21:1 on its tint, 5.99:1 on white.
- **Danger Tint** (`danger-tint`, #fdebec): the fill of the fault callout, the fault banner and a question's own fault line.
- **Danger Tint 2** (`danger-tint-2`, 16% of #d44c47): the fill behind the `body › questions › id` location code inside a fault.

### Dark scheme
The dark scheme follows `prefers-color-scheme` and redefines the same tokens (`color-scheme: light dark`, `theme-color` #ffffff and #191919). The blues (`accent`, `accent-fill`, `accent-fill-hover`, `on-accent`, `selection`) do not change; the tooltip inverts to a white card.

| Token | Light | Dark |
|---|---|---|
| `page` | #ffffff | #191919 |
| `panel` | #fbfbfa | #202020 |
| `raised` | #ffffff | #252525 |
| `code` | #f7f6f3 | #252525 |
| `ink` | #37352f | rgb(255 255 255 / 0.81) |
| `ink-2` | rgb(25 23 17 / 0.65) | #9b9b9b |
| `ink-3` | rgb(55 53 47 / 0.45) | #7f7f7f |
| `hairline` | #e9e9e7 | #2f2f2f |
| `hairline-strong` | rgb(55 53 47 / 0.16) | rgb(255 255 255 / 0.13) |
| `hover` | rgb(55 53 47 / 0.08) | rgb(255 255 255 / 0.055) |
| `pressed` | rgb(55 53 47 / 0.16) | rgb(255 255 255 / 0.11) |
| `field` | rgb(242 241 238 / 0.6) | rgb(255 255 255 / 0.04) |
| `tag` | rgb(227 226 224 / 0.5) | rgb(255 255 255 / 0.094) |
| `tag-ink` | #32302c | rgb(255 255 255 / 0.81) |
| `track` | rgb(55 53 47 / 0.14) | rgb(255 255 255 / 0.13) |
| `bar-muted` | #787774 | #9b9b9b |
| `accent-tint` | rgb(35 131 226 / 0.14) | rgb(35 131 226 / 0.22) |
| `danger` | #b3372f | #f28b88 |
| `danger-tint` | #fdebec | #362422 |
| `danger-tint-2` | rgb(212 76 71 / 0.16) | rgb(0 0 0 / 0.3) |
| `tooltip` | #0f0f0f | #ffffff |
| `tooltip-ink` | rgb(255 255 255 / 0.9) | #191919 |
| `overlay` | rgb(15 15 15 / 0.6) | rgb(15 15 15 / 0.8) |
| `scroll-thumb` | #d3d1cb | #454545 |

Dark contrast: the dark ink reads at 11.8:1 on #191919, `ink-2` #9b9b9b at 6.3:1 on the page and 5.5:1 on a raised block, and the dark danger #f28b88 at 6.16:1 on its tint #362422. The dark hairline shadows are 9.4% white at 1px; the popover and drawer keep their offsets with deeper black.

### Named Rules
**The Blue Is a Decision Rule.** Blue appears where something was decided or is about to be: the Run fill, the focus outline, text selection, and the bar and check of the winning row. It is never a heading, a link colour, a border or a background.

**The Red Is a Fault Rule.** Red marks a refusal or a destruction, nothing else: the fault callout, the fault banner, fault lines, an invalid field's hairline, the location code of a 422 issue, and the Remove question button. Emphasis, warnings and decoration never take it.

**The Hairlines, Not Borders Rule.** Nothing draws a CSS border in the normal schemes. An edge is a 1px shadow (`rgb(15 15 15 / 0.1) 0 0 0 1px` on blocks, an inset 16% ink line on fields) or a 1px inset divider in `hairline`; forced-colors mode is the one place borders are drawn.

## Typography

**Display Font:** the system sans (`ui-sans-serif`, -apple-system, BlinkMacSystemFont, Segoe UI, Helvetica, Arial)
**Body Font:** the same system sans
**Label/Mono Font:** the system mono (`ui-monospace`, SFMono-Regular, Menlo, Consolas, Liberation Mono, Courier)

**Character:** The type is the platform's own, set the way Notion sets it: 14px sans for everything the user reads or presses, 13px mono for everything the API reads or returns, one heavy 32px title, and no letter-spacing anywhere but that title. Tabular numerals are on for the whole body so figures in bar rows and property lists align.

### Hierarchy
- **Page title** (700, 2rem, 1.2, -0.01em): the one `Ardana playground` heading (the page's `h1`) beside its 32px page glyph. **Page title, compact** (700, 1.5rem, 1.2, -0.01em): the same heading below 720px, beside a 24px glyph.
- **Block heading** (600, 1rem, 1.3): the State and Questions headings, each question's id, and the Raw exchange and Snippets toggle headings.
- **UI** (500, 0.875rem, 1.2): buttons, sidebar rows, the top bar crumb, the workspace name (600), option names of the winning row (600).
- **Body** (400, 0.875rem, 1.5, tabular): the lede, instructions (max 65ch), option names, readouts, property names and values, field text, callout text (max 56ch).
- **Small** (500, 0.8125rem, 1.4): segments (line-height 1), small buttons, the banner, the toast, a question's fault line; at 400 for the toggle captions and fault lines under fields.
- **Label** (500, 0.75rem, 1.4): field labels, sidebar section headings (line-height 1), tags (line-height 1), the tooltip, criteria legends, the Sent and Received headings.
- **Caption** (400, 0.75rem, 1.4): the token count under the state, the last run line, field notes, the sidebar foot, the note under an option name, the score fit.
- **Code** (400, 0.8125rem, 1.6, mono): the state, the questions JSON, the wire panes, the snippet; 1.5 in a code field. Inline code is 0.85em of its line on the hover fill.

### Named Rules
**The System Face Rule.** No web font is loaded and none may be added; the page renders in whatever sans and mono the platform ships. Letter-spacing is off everywhere except the -0.01em of the 32px title. Nothing is set in uppercase.

**The Figures As Returned Rule.** Probabilities print as percent with one decimal, confidence, noul, score and extras with two decimals, counts and names verbatim; every figure carries its raw value and JSON pointer in `data-value` and `data-field`. Figures are tabular and right-aligned in a 4.5rem readout column.

## Layout

The shell is a two-column grid: a 240px sidebar and `minmax(0, 1fr)` for the main column, `min-height: 100vh`. The sidebar is sticky, full height, scrolls on its own, and carries a 1px inset hairline on its right edge. Collapsing it animates the first column to 0 over 200ms; the collapsed state is remembered in `localStorage` (`ardana.sidebar`) and the opener appears at the left of the top bar. Inside the sidebar: an 8px gutter, 16px between sections, 2px between rows, 24px section headings, 28px rows; the foot sits at the bottom with the serving origin and the Run shortcut.

The main column is a flex column: the 45px sticky top bar (12px side padding, inset hairline beneath, z-index 20), the banner beneath it (36px, only while it has text), then the page. The page is `max-width: 1400px`, centred, with 32px above, 48px at the sides and a 20vh tail so the last block can scroll under the bar; its blocks are 32px apart. The title row is a 32px glyph, a 12px gap and the heading, with the lede under it at 56ch.

The two columns of the page are `minmax(0, 5fr) minmax(0, 7fr)` with a 32px gap, aligned to the start: the state on the left, the questions on the right; that order is a product commitment. The state block is a code block textarea at `clamp(280px, 52vh, 600px)` tall with its tag in the block head and its token caption beneath. Question blocks are 12px apart with 16px padding; inside, 12px between the head, instructions, bars and properties. A bar row is the grid `16px minmax(6rem, 12rem) minmax(0, 1fr) auto` with 12px gaps and a 24px minimum height; a property row is `9rem minmax(0, 1fr)` at 28px. The raw exchange is two equal panes 16px apart. Builder rows are `2fr 3fr auto` for options and `1.5rem 1fr auto` for levels, 8px apart.

Spacing steps are 4, 8, 12, 16, 24, 32 and 48px; 2px and 6px appear as half steps inside controls (the segmented track's padding, gaps beside icons, the gap between bar rows).

Breakpoints, all `max-width`:
- **1200px**: page side padding drops to 32px.
- **960px**: the columns stack (24px gap) and the state shrinks to 220px; the sidebar becomes a fixed drawer at `min(240px, 85vw)` over a scrim, sliding in from the left over 200ms, with the page marked `inert` while it is open; the top bar's opener is always shown; after a run the page scrolls the fault or the first question to the top.
- **720px**: page padding 16px and gaps 24px; the title falls to 1.5rem and its glyph to 24px; question padding 12px; bar rows restack to `mark name value` over `. bar bar`; option rows put the description on a second line; the exchange panes stack; the share popover becomes fixed under the top bar with 12px side margins.

Anything the page scrolls to (`.page`, `.question`, `.callout`, `.toggle`) has `scroll-margin-top` of the bar plus 12px, so a target lands under the sticky bar and not behind it.

## Elevation & Depth

The system is flat with hairlines. Blocks that sit on the page (question blocks) get a 1px shadow ring, not a border; the sidebar, the top bar, the banner, a property list and a builder panel are separated by 1px inset hairlines. Only things that float above the page cast an offset shadow: the share popover, the drawer, the toast, the skip link, and the 1px lift of a checked segment. The dark scheme keeps the same shapes and swaps the 1px ring to 9.4% white.

### Shadow Vocabulary
- **Hairline ring** (`box-shadow: rgb(15 15 15 / 0.1) 0 0 0 1px`): question blocks. Dark: `rgb(255 255 255 / 0.094) 0 0 0 1px`.
- **Field inset** (`box-shadow: inset 0 0 0 1px var(--hairline-strong)`): inputs, the select, an unchecked check box.
- **Focus ring** (`box-shadow: inset 0 0 0 1px var(--accent), 0 0 0 2px var(--accent-tint)`): a focused field, select or code block; every other control takes a 2px `accent` outline at 1px offset.
- **Fault inset** (`box-shadow: inset 0 0 0 1px var(--danger)`): a field or code block whose value was refused (`aria-invalid`).
- **Divider** (`box-shadow: inset -1px 0 0 var(--hairline)` / `inset 0 -1px 0` / `inset 0 1px 0`): the sidebar's right edge, the top bar's and banner's bottom edge, the top of a property list or builder panel.
- **Segment lift** (`box-shadow: 0 0 0 1px var(--edge-strong), rgb(15 15 15 / 0.1) 0 1px 2px`): the checked segment on its raised plate; the ring is `edge-strong` (55% ink, 45% white in dark), which holds 3:1 on the track so the picked state is not colour alone.
- **Popover** (`box-shadow: rgb(15 15 15 / 0.1) 0 0 0 1px, rgb(15 15 15 / 0.1) 0 3px 6px, rgb(15 15 15 / 0.2) 0 9px 24px`): the share popover and the skip link. Dark: the ring at 9.4% white, the offsets at 50% and 60% black.
- **Drawer** (`box-shadow: rgb(15 15 15 / 0.2) 0 0 0 1px, rgb(15 15 15 / 0.3) 0 12px 32px`): the sidebar as a drawer below 960px. Dark: ring at 9.4% white, offset at 70% black.
- **Toast** (`box-shadow: rgb(15 15 15 / 0.3) 0 4px 16px`): the copy toast at the bottom right.

### Named Rules
**The Floating Only Rule.** A block on the page never casts an offset shadow; it gets the 1px hairline ring or a divider. Offset shadows belong to surfaces that float over the page: the popover, the drawer, the toast, the skip link, and the checked segment's 1px lift.

## Shapes

Corners are small and stepped by role: 3px on the smallest things (tags, the check box, the bars, inline code, a defined term's focus shape), 4px on inputs, the select, rows, segments, the workspace mark and a question's fault line, 6px on buttons, question blocks, callouts, the segmented track, the popover, the toast and the skip link, 8px on code blocks and the empty wire panes, and 10px on the tooltip. Nothing is a pill and nothing is a circle except the icons' arcs.

There are no borders in the normal schemes; edges are shadows and dividers (see Elevation). The bar is a 6px track with a 3px radius whose fill grows to the returned probability. The check box is a 16px square with a 12px check glyph at stroke 2, drawn only when the row is the answer. Icons are drawn on one 16-unit grid at 16px (14px in the select and the small Run spinner), one 1.5px stroke with round caps and joins in the current colour; the only filled glyph is the toggle triangle. The workspace mark is a 20px square of ink with a white "A".

## Components

Quiet at rest, revealed on hover: every control is a transparent shape that fills at 8% ink when the pointer arrives, and a decided one is blue. Hover fills, colours and shadows transition over 100ms `ease-in`; the shell and the drawer over 200ms on `cubic-bezier(0.3, 0, 0.5, 1)`; all transitions and animations are off under `prefers-reduced-motion`. In forced-colors mode buttons, fields, tags and blocks draw a 1px `CanvasText` border, the primary button and bar fills use `Highlight`, and focus uses `Highlight`.

### Buttons
- **Shape:** 6px radius, 28px tall, 500 at 14px, 6px between icon and label, icons in `ink-2` that turn to ink on hover.
- **Ghost (default):** transparent; hover 8% ink; active or `aria-expanded="true"` 16% ink; a held button (`aria-disabled="true"`) turns its label `ink-3` and its cursor default but stays focusable. Share, Edit, Copy, Add option and the sidebar's rows are ghosts.
- **Primary (Run):** `accent-fill` with white text, 12px side padding; hover and active `accent-fill-hover`; held at 50% opacity with its reason in the banner under the top bar (`aria-describedby`); busy at 85% opacity with a 14px spinner arc turning at 900ms and the label reading Running or, while the server pulls the model, Pulling. It announces `Control+Enter Meta+Enter` in `aria-keyshortcuts` and carries a tooltip with the shortcut.
- **Icon:** a 28px square (24px in the small size) whose name is its `aria-label` and its tooltip: the sidebar's opener and closer, Remove option and Remove level.
- **Small:** 24px tall at 13px for row-level actions: Edit, Copy, Add option, Add level, Remove question (in `danger`).
- **Focus:** a 2px `accent` outline at 1px offset on every button.

### Segmented control
A native radio group (`role="radiogroup"` labelled by its legend) in a 6px track of 8% ink with 2px padding. Each segment is a label 24px tall with 10px side padding, 13px at 500 in `ink-2`, hovering to ink; the checked segment sits on a `raised` plate with the segment lift shadow and reads in ink. The real radio covers its segment invisibly so arrow keys move the choice; focus shows as the 2px `accent` outline on the segment. Used for a question's type (noul, choice, score) and the snippet language.

### Fields
- **Style:** 28px tall, 3px 8px padding, 4px radius, `field` fill under a 1px inset `hairline-strong`; 14px at 1.5; placeholders in `ink-2`; code fields (question id, option names, the share link) in 13px mono. A 12px label at 500 in `ink-2` sits 4px above; a 12px note or a 13px fault line sits beneath.
- **Focus:** the inset ring turns blue and a 2px `accent-tint` halo appears outside it; the outline is dropped.
- **Error:** `aria-invalid="true"` turns the inset ring `danger` and the fault line beneath it reads the message in `danger`; the message is announced politely only when the field turns invalid or valid again.
- **Disabled:** a field the builder cannot edit (structured JSON) is disabled, transparent, in `ink-2`, with a note saying to edit it in the questions JSON.
- **Select:** the model picker is a real `select` dressed as a field: transparent with the inset hairline, 28px right padding for a 14px chevron in `ink-2`, hover fill, the same focus ring; pulled models first, then the library models with their download size.

### Tags
A 20px chip with 6px side padding, 3px radius, 12px at 500, `tag` fill and `tag-ink`: "Sent as text" or "Sent as JSON" in the state's head, "Inputs changed" in the top bar and "From last run · inputs changed" on a stale question and in the last-run line.

### Tooltip
A dark card (`tooltip`, white in the dark scheme) with 4px 8px padding, 10px radius, 12px at 500, centred text, at most `min(272px, 60vw)` wide, rendered from `data-tip` as `::after` above, below, at the start or at the end of its control. A second line carries the shortcut ("Ctrl+Enter or ⌘↵"). It appears on hover, on focus-visible, or when a child has focus; a transparent 6px border bridges the gap so the pointer can cross onto it; Escape dismisses every tooltip (`tips-off` on the root) until the pointer or focus moves again. A tooltip describes; it is never the only name of a control. Property names (Confidence, P max, Certainty) are `dfn` terms with a help cursor and a tooltip that states their meaning, with the same text visually hidden for readers.

### Toast
The copy button's confirmation: a fixed card 12px from the bottom right, `tooltip` fill and ink, 8px 12px padding, 6px radius, 13px at 500, the toast shadow, in a `role="status"` span; it says what was copied and clears after 4 seconds.

### Banner
The line under the top bar: 36px minimum, 6px 16px padding, 13px at 500, a hairline beneath, hidden when empty. Quiet (`panel` fill, ink) for a wait or a hint: why Run is held, or what a first run is downloading and its size; fault colours (`danger` on `danger-tint`) when the questions JSON does not parse. It is the Run button's description.

### Navigation (sidebar and top bar)
- **Sidebar:** `panel` fill, 240px, an inset hairline at the right; the workspace row (20px ink mark, "Ardana" at 600 as plain text, and a close control that is invisible until the row is hovered or the sidebar has focus), then the Model select, Presets as page rows, "On this page" rows, and the foot in 12px `ink-2`.
- **Row:** 28px, 8px side padding, 4px radius, 14px at 500 in ink with a 16px `ink-2` glyph; hover 8% ink, active 16%; `aria-current="true"` keeps the hover fill on the section in view (the last section whose top has passed 96px). Presets load the state and questions and offer a "Restore previous" row in `ink-2`; section rows open a closed toggle, scroll to it and move focus onto it.
- **Top bar:** 45px, `page` fill, hairline beneath; the opener (only while the sidebar is collapsed or a drawer), the page crumb (16px glyph, name at 500), then the "Inputs changed" tag, Share and Run. Share and Run never shrink; the tag gives way first, then the crumb.
- **Collapse:** Ctrl/Cmd+\ or the two controls; 200ms on the grid columns; remembered in `localStorage`; focus moves to the opener on close and to the closer on open.
- **Drawer (below 960px):** the sidebar fixed at the left over a 60% black scrim, sliding in over 200ms with the drawer shadow; the page behind is `inert`; Escape or the scrim closes it; the opener carries `aria-expanded` and `aria-controls`; a pick inside closes it and moves focus to the state.

### Question block (signature)
A `raised` block with the hairline ring, 6px radius, 16px padding (12px below 720px), 12px between its parts. The head puts the question id (16px at 600) beside the Type segmented control and the Edit ghost (`aria-expanded`, `aria-controls` on the builder panel); the instructions follow in `ink-2` at 65ch. Edit opens the builder under a hairline: id and instructions fields, then option rows (name, optional description, Remove) or level rows (number, description, Remove) with a legend counting them against their bounds, Add held with `aria-disabled` at the upper bound and Remove at the lower, and Remove question in `danger` at the right.
- **Bar row:** a 16px check box (inset hairline; blue with a white check when the row is the answer; hidden for a noul row, which is a probability, not a pick), the option name (600 on the winner, with a 12px `ink-2` note beneath for score legends and "probability of yes"), the 6px bar whose fill grows from 0 to the probability over 400ms on the standard ease, and the exact figure right-aligned in a 4.5rem tabular column. Before a run the rows show the question's options on empty bars with an `–` readout and a hidden "not run yet". A score row adds "fit" and its percent under the readout.
- **Stale:** when the inputs no longer match the run, the figures stay but the winner's blue bar and check go `bar-muted`, the readouts go `ink-2`, and the tag says so.
- **Fault:** a 422 issue that names this question appears under the head as a 13px line on `danger-tint` with a `danger` alert glyph, while the question is as it was sent.
- **Property list:** under a hairline, one 28px row per property: a 16px `ink-3` glyph, the name in `ink-2` as a `dfn` with its tooltip, the value in ink (the Answer at 600). An answer of an unknown type is shown as pretty JSON in a code block.
- **Add question:** the "+ Add question" ghost row under the list, in `ink-2` until hovered, like a database's New row; it opens the new question's builder with focus on its id field.

### Callouts
- **Plain:** `code` fill, 6px radius, 16px padding (12px at the left), an `ink-2` glyph beside text at 56ch: the empty-list callout with inline `noul`, `choice` and `score`.
- **Danger:** `danger-tint` fill; the alert glyph and the title ("HTTP 422 · the request was not answered.") in `danger` at 600; each issue as a location code (`body › questions › id`, `danger` on `danger-tint-2`, 3px radius) followed by its message in ink; an unreadable detail as a code block. It stands at the top of the questions column after a failed run.

### Toggle blocks
Native `details` open by default, for the Raw exchange and the Snippets. The summary is a fit-content row with a 4px radius and the hover fill: a 24px marker holding the filled triangle (turning 90° over 100ms when open), the heading at 16px 600, and a 13px caption in `ink-2` ("Last run, byte for byte", "The request as it stands"). Its content is 12px below: two wire panes, or the Language segmented control and Copy over the snippet.

### Code blocks
`code` fill, 8px radius, 13px mono at 1.6, 2-space tabs, 16px padding. As a textarea (the state at `clamp(280px, 52vh, 600px)`, the questions JSON at 200px) it resizes vertically, takes the blue focus ring and the `danger` ring when invalid, and has no visible edge at rest. As a `pre` (the sent and received bodies, the snippet) it wraps, is focusable (`tabindex="0"`) with an `aria-label`, and shows the bytes as they went over the wire. An empty pane is a 13px `ink-2` sentence on the same fill.

### Popover
The share panel under the Share button: `raised`, 6px radius, 12px padding, 8px between its rows, the popover shadow, `min(420px, calc(100vw - 24px))` wide and right-aligned to the button (fixed under the top bar at 12px margins below 720px). It holds a 12px label, a read-only mono field that selects itself on focus, a Copy button, and a 12px note. Opening moves focus into the field; Escape closes it and returns focus to the button; a click outside closes it.

### Skip link and status
A "Skip to the page" link sits at 8px, 8px, hidden above the viewport until focused, on a `raised` card with the popover shadow. Two visually hidden `role="status"` regions announce the last run's outcome ("Answered by decider-2b-v11: 2 questions, 112 ms") and the builder's field notices; the share-link error and a failed model list are `role="alert"` fault lines. Headings run h1 (the page title) → h2 (State, Questions, Raw exchange, Snippets) → h3 (question ids, the Sent and Received captions).

## Do's and Don'ts

### Do:
- **Do** set every control at 28px (24px inside a row), 14px at 500, transparent at rest, 8% ink on hover, 16% when pressed or expanded.
- **Do** use `aria-disabled="true"` on a held button, never `disabled`, so it keeps focus and its tooltip; put the reason in the banner or beside the control.
- **Do** separate surfaces with hairlines: the 1px ring on blocks, the inset 16% ink line on fields, the 1px inset divider between regions.
- **Do** show a returned probability as a bar row: check, name, 6px bar grown over 400ms, the exact figure tabular at the right; the winner alone in blue.
- **Do** keep every figure at its stated precision with `data-field` and `data-value` attached, and grey it (not remove it) when the inputs move on.
- **Do** keep hover, colour and shadow transitions at 100ms `ease-in`, the shell and drawer at 200ms, and turn every transition and animation off under `prefers-reduced-motion`.
- **Do** define a token once in `:root` and redefine it under `prefers-color-scheme: dark`; the page offers no theme switch.

### Don't:
- **Don't** load a web font, add letter-spacing to UI text, or set anything in uppercase; the system sans and mono are the type.
- **Don't** use blue for anything but the Run fill, focus, selection and the winning row; never as a heading, link, border or background.
- **Don't** use red outside a fault or the Remove question action; no red emphasis, warning or decoration.
- **Don't** draw a CSS border in the light or dark scheme; edges are hairline shadows, and borders belong to forced-colors mode only.
- **Don't** give a block on the page an offset shadow, a gradient, an engraved legend, a glowing readout or any hardware metaphor.
- **Don't** put a tooltip where a visible name belongs, and never name a control only by its tooltip.
- **Don't** round, restyle or re-derive a returned value; show it as the API said it.
