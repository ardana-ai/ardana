---
name: Ardana
description: A cassette-futurism tape-deck fascia for running a state and questions against a local System 1 decision model.
colors:
  amber: "#ffb000"
  amber-dim: "#cc7a00"
  amber-off: "#3d2e0d"
  amber-ghost: "#282010"
  fault-ink: "#8f1d17"
  fault-lamp: "#e0301e"
  fault-paper: "#f7e4df"
  surround: "#e6dfc9"
  surround-shade: "#cfc6aa"
  alu-hi: "#e4e7ea"
  alu: "#c7ccd1"
  alu-lo: "#a9afb5"
  chrome-hi: "#f7f8f9"
  chrome-lo: "#b0b6bc"
  ink: "#1b1b1b"
  ink-soft: "#35383b"
  engrave: "rgb(255 255 255 / 0.6)"
  glass: "#15140f"
  glass-text: "#e9e4d4"
  paper: "#f4efe0"
  paper-shade: "#e3dcc7"
  paper-hint: "#5e584a"
  shell: "#282620"
  shell-hi: "#3a372f"
  bevel-deep: "#050403"
  bevel-side: "#1d1c17"
typography:
  display:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "2.25rem"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.08em"
  headline:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.1
    letterSpacing: "0.1em"
  title:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1.25rem"
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: "0.02em"
  body:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.45
    fontFeature: "tnum"
  body-small:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, sans-serif"
    fontSize: "0.9375rem"
    fontWeight: 400
    lineHeight: 1.4
  label:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0.12em"
  key-legend:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.16em"
  switch:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0.04em"
  figure:
    fontFamily: "Barlow Condensed, Arial Narrow, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    letterSpacing: "0.04em"
  code:
    fontFamily: "IBM Plex Mono, ui-monospace, monospace"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.55
  code-small:
    fontFamily: "IBM Plex Mono, ui-monospace, monospace"
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.55
  matrix:
    fontFamily: "Doto, IBM Plex Mono, ui-monospace, monospace"
    fontSize: "1.75rem"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "0.04em"
  matrix-readout:
    fontFamily: "Doto, IBM Plex Mono, ui-monospace, monospace"
    fontSize: "1.25rem"
    fontWeight: 800
    lineHeight: 1.1
rounded:
  fascia: "16px"
  module: "10px"
  control: "6px"
  window: "4px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "5": "24px"
  "6": "32px"
components:
  fascia:
    backgroundColor: "{colors.alu}"
    textColor: "{colors.ink}"
    rounded: "{rounded.fascia}"
  run-key:
    backgroundColor: "{colors.chrome-hi}"
    textColor: "{colors.ink}"
    typography: "{typography.key-legend}"
    rounded: "{rounded.control}"
    padding: "14px 10px 10px"
    width: "104px"
    height: "80px"
  run-key-disabled:
    textColor: "{colors.ink-soft}"
  model-switch:
    backgroundColor: "{colors.chrome-hi}"
    textColor: "{colors.ink}"
    typography: "{typography.switch}"
    rounded: "{rounded.control}"
    padding: "10px 40px 10px 14px"
  counter-window:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.amber}"
    typography: "{typography.matrix}"
    padding: "6px 12px 4px"
  counter-window-idle:
    textColor: "{colors.amber-off}"
  cassette:
    backgroundColor: "{colors.shell}"
    rounded: "{rounded.module}"
    padding: "{spacing.4}"
  cassette-label:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    typography: "{typography.headline}"
    rounded: "{rounded.window}"
    padding: "8px 12px"
  tape:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    typography: "{typography.code}"
    rounded: "{rounded.window}"
    padding: "12px 16px"
  channel:
    backgroundColor: "{colors.alu}"
    textColor: "{colors.ink}"
    rounded: "{rounded.module}"
    padding: "{spacing.4}"
  ladder:
    backgroundColor: "{colors.glass}"
    height: "16px"
  ladder-segment-lit:
    backgroundColor: "{colors.amber}"
    width: "6px"
    height: "10px"
  ladder-segment-unlit:
    backgroundColor: "{colors.amber-off}"
    width: "6px"
    height: "10px"
  readout:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.amber}"
    typography: "{typography.matrix-readout}"
    rounded: "{rounded.window}"
    padding: "3px 8px 1px"
  readout-idle:
    textColor: "{colors.amber-off}"
  figure-text:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.amber}"
    typography: "{typography.figure}"
    rounded: "{rounded.window}"
    padding: "3px 10px"
  lamp:
    backgroundColor: "{colors.amber-off}"
    size: "8px"
  lamp-lit:
    backgroundColor: "{colors.amber}"
    size: "8px"
  wire:
    backgroundColor: "{colors.glass}"
    textColor: "{colors.glass-text}"
    typography: "{typography.code-small}"
    rounded: "{rounded.control}"
    padding: "12px 16px"
  fault:
    backgroundColor: "{colors.fault-paper}"
    textColor: "{colors.fault-ink}"
    rounded: "{rounded.module}"
    padding: "{spacing.4}"
---

# Design System: Ardana

## Overview

**Creative North Star: "The Tape-Deck Fascia"**

The playground is one piece of consumer hardware: a brushed-aluminium fascia set into a beige plastic surround, lit from above. The state loads into a dark cassette shell behind a paper label; each question is a bolted channel module; every option's probability climbs an amber LED ladder behind smoked glass and lands on a dot-matrix readout. Nothing on the fascia is decoration without a hardware reason: legends are engraved, windows are sunk, keys travel, and lamps are dark until something is live.

Density is instrument-panel density: small engraved caps under every control, generous module padding, and figures set large enough to read at a glance. The world is light-only (`color-scheme: light`); the beige surround and aluminium are the page, and the only dark surfaces are windows into the machine (glass, the cassette shell, the wire). The confirmed rejection is the two-pane API console of grey borders and a blue Run button: no flat grey rules, no blue, no generic primary button.

Amber is signal, not paint. It appears where the machine has something to say (a returned figure, the winning answer, the busy RUN strip) and nowhere else. Red is spent on faults only.

**Key Characteristics:**
- Aluminium fascia on beige surround, lit from above; every surface has a light-from-above reason.
- Engraved narrow-grotesk caps (Barlow Condensed) as the legend under or beside every control and window.
- Smoked-glass windows with a sunk bevel for every live figure; amber dot-matrix numerals (Doto) inside.
- Amber LED ladders on an 8px segment pitch with a quarter-tick scale beneath.
- One pressable chrome transport key (RUN) with real key travel.
- Red reserved for faults; amber reserved for live signal.

## Colors

A warm-neutral hardware palette (beige plastic, cool aluminium, cream paper, near-black smoked glass) with one signal hue, amber, and one fault hue, red.

### Primary
- **LED Amber** (#ffb000): lit ladder segments, dot-matrix figures, lit lamp cores, the RUN strip while busy, text selection, and the focus ring on the paper editors. Every amber figure is a value the API returned.
- **Amber Filament** (#cc7a00): the dim edge of a lit lamp, the lit RUN strip's border, and the 2px underline under the winning option's name.
- **Unlit Amber** (#3d2e0d): dark lamps, unlit ladder segments, idle readout and counter dots. Never carries a readable figure.
- **Ghost Matrix** (#282010): the unlit 5x7 dot grid behind every dot-matrix window. It is inlined in the grid's SVG data URI (a custom property cannot enter a data URI), so change both together.

### Secondary
- **Fault Ink** (#8f1d17): fault titles, fault-line text, the invalid border on the questions editor, and the fault lamp's rim.
- **Fault Lamp** (#e0301e): the core of the fault lamp only.
- **Fault Paper** (#f7e4df): the pale red ground of fault panels and the share-link fault line.

### Neutral
- **Beige Surround** (#e6dfc9): the page behind the fascia; also the browser theme colour.
- **Surround Shadow** (#cfc6aa): scrollbar track on the page.
- **Aluminium** (#c7ccd1): the fascia body and channel modules.
- **Aluminium Highlight** (#e4e7ea): the fascia's top edge, the light half of every groove, and the bottom lip of every sunk window.
- **Aluminium Shade** (#a9afb5): the dark half of every groove, ladder tick marks, scrollbar thumbs.
- **Chrome Highlight** (#f7f8f9) and **Chrome Shade** (#b0b6bc): the top and bottom stops of the chrome gradients on the RUN key and the model switch.
- **Engraving Ink** (#1b1b1b): legends, headings and body text on aluminium and paper; the global focus outline.
- **Soft Engraving** (#35383b): secondary legends, instructions, notes, disabled key text.
- **Engrave Light** (rgb(255 255 255 / 0.6)): the 1px under-cut light that makes a legend read as engraved.
- **Smoked Glass** (#15140f): readouts, counters, ladders, the wire, raw answers.
- **Glass Text** (#e9e4d4): monospace text on glass (the wire and raw answers).
- **Label Paper** (#f4efe0), **Paper Shade** (#e3dcc7), **Paper Hint** (#5e584a): the cassette label and the two editors; paper hint is placeholder text and unlit mode-lamp captions.
- **Cassette Shell** (#282620) and **Shell Highlight** (#3a372f): the cassette body and its lit top.
- **Bevel Deep** (#050403) and **Bevel Side** (#1d1c17): the top and side walls of every sunk window.

### Named Rules
**The Amber Is Live Rule.** Amber marks live signal only: returned figures, the winning answer's lamp and underline, the busy RUN strip, selection and editor focus. It is never a border, a background panel, a heading colour or an ornament.

**The Red Is a Fault Rule.** The three fault colours appear only when a request or input was refused. No red for emphasis, warnings or destructive-looking controls.

**The Unlit Is Not Text Rule.** Unlit Amber and Ghost Matrix sit at about 1.4:1 on glass by design; they draw dark dots and segments only. An idle window shows an empty cell with a visually hidden "not run yet" / "none yet", never dim digits.

## Typography

**Display Font:** Barlow Condensed 500/600 (with Arial Narrow, sans-serif)
**Body Font:** Barlow Semi Condensed 400/500 (with Arial Narrow, sans-serif)
**Label/Mono Font:** IBM Plex Mono 400 for editors and the wire; Doto 800 for dot-matrix figures

**Character:** Narrow machine-plate caps for everything engraved, a slightly wider sibling for anything a person reads as prose, a plain mono for anything a person copies, and a dot-matrix face for anything the machine reports. All four are self-hosted woff2 under the SIL OFL, with Barlow Condensed 600 and Doto preloaded.

### Hierarchy
- **Display** (Barlow Condensed 600, 2.25rem, 1, 0.08em, uppercase): the maker's plate "Ardana" in the rail. One per page.
- **Headline** (Barlow Condensed 600, 1.125rem, 1.1, 0.1em, uppercase): engraved panel headings (State, Questions, Raw exchange).
- **Title** (Barlow Condensed 600, 1.25rem, 1.15, 0.02em, sentence case as typed): a channel's question id, shown exactly as the id in the request.
- **Body** (Barlow Semi Condensed 400, 1rem, 1.45, tabular numerals): running text and option names (500 for option names).
- **Body Small** (Barlow Semi Condensed 400, 0.9375rem, 1.4, max 65ch): channel instructions; 0.875rem for option notes and level-fit lines.
- **Label** (Barlow Condensed 600, 0.75rem, 1.2, 0.12em, uppercase): engraved legends under controls and windows, mode-lamp captions, the "Type" key. The maker's line uses the 500 weight at 0.14em.
- **Key Legend** (Barlow Condensed 600, 0.875rem, 1, 0.16em, uppercase): the RUN key's legend.
- **Switch** (Barlow Condensed 600, 1rem, 1.2, 0.04em): the model name on the chrome switch; 0.9375rem for the "answered by" model.
- **Figure** (Barlow Condensed 600, 1.125rem, 0.04em, amber on glass): returned text values such as the chosen answer.
- **Code** (IBM Plex Mono 400, 0.875rem, 1.55): the state tape and the questions editor. **Code Small** (0.8125rem, 1.55) for the wire and raw answers.
- **Matrix** (Doto 800, 1.75rem, 1, 0.04em, amber): rail and cassette counters (1.375rem at phone width). **Matrix Readout** (Doto 800, 1.25rem, 1.1): per-option percent readouts and figures (1.125rem at phone width).

### Named Rules
**The Four Faces Rule.** Each face has one job: Barlow Condensed engraves, Barlow Semi Condensed reads, IBM Plex Mono is copied, Doto reports. Doto never sets words; Plex never sets legends.

**The Engraved Legend Rule.** Every control and every window carries a legend in the Label role, cut into the panel with a 1px Engrave Light under-shadow. On paper or on the dark shell the under-shadow is dropped, since there is no aluminium to cut into.

**The Doto Point Rule.** Doto's own full stop is a cross of dots that reads as "+". Every decimal point in a dot-matrix figure is set in its own span that keeps the glyph cell transparent and seats one square dot (0.085em on the 0.1em pitch) on the baseline, so the text stays exact and the point reads as a point.

**The Figures As Returned Rule.** Probabilities show as percent with one decimal; confidence, noul, score and extras with two decimals; counts and names verbatim. Each figure carries its raw value and JSON pointer, and no figure is restyled into something the API did not say.

## Layout

One fascia, max 1440px, centred on the surround with 24px of page padding (8px at 720px and below). Inside it, a top rail and a deck.

The rail is a four-area grid: maker's plate, model switch (12rem to 22rem), counters (right-aligned) and the RUN key at the far right, with 16px/32px gaps and 16px/24px padding. At 1080px the counters drop to a second full-width row; at 720px the rail becomes maker + RUN, then the switch, then three equal counter windows.

The deck is a two-column grid, state cassette left (5fr) and question channels right (7fr), with the raw exchange spanning both below; 24px gap and padding. At 960px it becomes one column in reading order (cassette, channels, exchange); at 720px gaps tighten to 16px and padding to 12px. The state-left, questions-right order is binding.

Spacing runs on a 4px base: 4, 8, 12, 16, 24, 32. Stacks inside modules use 12px; module padding is 16px (12px at phone width). The ladder row is a four-track grid (8px lamp, 6rem to 13rem name, flexible ladder, readout); at phone width the ladder drops beneath the name and the readout stays on the name's line. Figures under the ladders wrap as a row, and become a two-column grid at phone width.

## Elevation & Depth

Depth is physical and always lit from above: raised parts catch a white top highlight and cast a short soft shadow; sunk windows show a dark top wall and a light bottom lip. There are three depths: modules that sit proud of the fascia, the fascia itself, and windows sunk into it. Grooves (a 1px Aluminium Shade line over a 1px Aluminium Highlight line) replace every divider rule.

### Shadow Vocabulary
- **Engraving** (`text-shadow: 0 1px 0 rgb(255 255 255 / 0.6)`): legends and headings cut into aluminium.
- **Groove** (`box-shadow: 0 1px 0 #a9afb5, 0 2px 0 #e4e7ea`): the rule under the rail and under panel heads; inset variant above the figures row.
- **Chrome plate** (`box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.95), inset 0 -1px 0 rgb(0 0 0 / 0.18), 0 1px 2px rgb(0 0 0 / 0.3)`): the model switch.
- **Channel module** (`box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.8), 0 2px 4px rgb(0 0 0 / 0.18), 0 6px 12px -8px rgb(0 0 0 / 0.35)`): bolted question modules.
- **Cassette** (`box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.12), 0 8px 16px -8px rgb(0 0 0 / 0.55), 0 1px 2px rgb(0 0 0 / 0.35)`): the heavier cassette shell.
- **Fault panel** (`box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.6), 0 2px 4px rgb(0 0 0 / 0.18)`).
- **Key travel** (rest `inset 0 1px 0 rgb(255 255 255 / 0.95), inset 0 -2px 0 rgb(0 0 0 / 0.2), 0 3px 0 #7d838a, 0 5px 10px rgb(0 0 0 / 0.3)`; pressed: translateY(3px) and the 3px skirt collapses to 0): the RUN key only.

### Named Rules
**The Sunk Window Rule.** Every glass window (counters, readouts, figures, ladders, the wire, the cassette window) is sunk with a bevel border: top 2px (3px for the larger wells) Bevel Deep, sides 1px Bevel Side, bottom 1px Aluminium Highlight. Windows never cast shadows.

**The Key Travel Rule.** The 3px solid skirt under the RUN key is the key's travel, consumed when it is pressed or busy. It belongs to pressable transport keys only; nothing else gets a hard offset shadow.

## Shapes

Softened hardware corners, stepped by scale: the fascia at 16px (10px at phone width), modules (cassette, channels, fault, empty states) at 10px, controls (RUN key, switch, wire) at 6px, and windows and labels (readouts, figure text, tape, cassette label, fault lines) at 4px. Lamps are circles (8px, the fault lamp 10px). The fascia has a 4px bottom border as its thickness; the cassette carries four screw heads drawn as radial gradients in its corners. Ladders are segmented, not continuous: 6px by 10px segments on an 8px pitch, with a five-tick scale (0, 25, 50, 75, 100%, alternating full and half height) cut beneath.

## Components

### Buttons (RUN transport key)
The only button on the fascia: a chrome transport key, tactile and deliberate.
- **Shape:** gently squared (6px), 104 by 80px (88 by 68px at phone width).
- **Primary:** chrome gradient (Chrome Highlight to Chrome Shade), Engraving Ink play glyph (22px inline SVG) over the "Run" key legend, with a 4px LED strip across the top.
- **Hover:** the chrome brightens one step. **Active / busy:** the key drops 3px, its skirt collapses, the LED strip burns amber and the legend reads "Running"; `aria-busy` holds that pressed state for the whole run. Transitions are 80ms on `cubic-bezier(0.16, 1, 0.3, 1)`, none under reduced motion.
- **Disabled:** flat pale chrome, Soft Engraving legend, not-allowed cursor.
- **Focus:** 2px Engraving Ink outline, 4px offset. Ctrl/Cmd+Enter in either editor presses it.

### Model Switch
- **Style:** a real `select` on a chrome plate (6px), model name in the Switch role, a 2px engraved chevron at the right, the "Model" legend engraved beneath. After a run, an "Answered by" legend shows the model the API reported.
- **Focus:** Engraving Ink outline at 3px offset.

### Counters and Readouts (dot-matrix windows)
- **Style:** Smoked Glass window with the Sunk Window bevel; amber Doto digits right-aligned over an unlit 5x7 ghost grid, one 0.6em cell per character, so lit dots land on ghost dots. Counters (latency ms, input and output tokens) carry an engraved legend beneath; per-option readouts sit at the end of each ladder row (min 5.25rem).
- **Idle:** an empty cell over the ghost grid, with a visually hidden "none yet" or "not run yet".

### Cassette (state input)
- **Style:** dark shell (Cassette Shell with a Shell Highlight top), 10px corners, four screw heads, the Cassette shadow. A cream paper label strip carries the engraved "State" heading and two mode lamps (Text, JSON) whose lit lamp says how the state will be sent.
- **Window:** a near-black well with a 3px bevel holding the tape, a real text area on Label Paper in Code type (min height clamp(260px, 50vh, 560px); 220px in one column). Focus is a 2px amber outline.
- **Foot:** the input-token counter, with its legend in Label Paper on the shell.

### Channel (question module)
- **Corner Style:** 10px. **Background:** an aluminium gradient lighter at the top. **Shadow:** Channel module. **Padding:** 16px (12px at phone width).
- **Head:** the question id in the Title role, instructions in Body Small beneath, and a read-only engraved type legend ("Type" key + type name) at the right, dropping under the title at phone width.
- **Ladder row:** lamp, option name, ladder, readout. The winning option's lamp lights amber and its name takes a 2px Amber Filament underline at 4px offset, plus a visually hidden "(answer)".
- **Ladder:** Smoked Glass track (16px high, 3px corners, sunk bevel) of unlit segments; the lit fill is clipped to the returned level and climbs from zero in 900ms on `cubic-bezier(0.16, 1, 0.3, 1)`, static under reduced motion.
- **Figures:** under a groove, engraved legends over glass windows: the answer as Figure text, confidence, p max, certainty and other extras as readouts.

### Inputs / Fields (questions editor)
- **Style:** Label Paper text area in Code type inside a 6px near-black frame (8px corners), with the engraved "Questions JSON" legend above.
- **Focus:** 2px amber outline, 2px offset.
- **Error:** the frame turns Fault Ink and a Fault Ink line reports the parse error beneath.

### Wire (raw exchange)
- **Style:** Smoked Glass panes (6px, 3px bevel) in Code Small, Glass Text, pre-wrapped, max 22rem tall and scrollable, focusable. Two side by side (sent, received with HTTP status), stacked at phone width. Selection inside the wire is amber on glass.
- **Empty:** a recessed aluminium plate (6px, 2px top bevel in Aluminium Shade) with Soft Engraving text saying what will appear.

### Fault
- **Style:** Fault Paper panel (10px) with Fault Ink text, a red fault lamp beside the title, and each issue as its location in mono Fault Ink followed by the message in Engraving Ink 500. Errors show as the API reported them.

### Panel Head
- **Style:** the engraved Headline for the panel, followed by a Groove rule and 12px before the content.

## Do's and Don'ts

### Do:
- **Do** set every live figure in amber Doto inside a sunk Smoked Glass window over the ghost grid, and set decimal points with the Doto Point treatment.
- **Do** engrave a Label legend (Barlow Condensed 600, 0.75rem, 0.12em, uppercase, 1px Engrave Light under-shadow) under or beside every control and window.
- **Do** separate regions with the two-line Groove (Aluminium Shade over Aluminium Highlight), never a flat border.
- **Do** light surfaces from above: white top highlights on raised parts, dark top walls on sunk windows.
- **Do** keep motion to the RUN key press (80ms) and the ladder climb (900ms), both on `cubic-bezier(0.16, 1, 0.3, 1)` and both off under reduced motion.
- **Do** keep the state on the left and the questions on the right; restack as cassette, channels, exchange in one column.

### Don't:
- **Don't** build the two-pane API console of grey borders and a blue Run button; no blue anywhere on the fascia.
- **Don't** use amber for borders, panel fills, headings or decoration; it marks live signal only.
- **Don't** use the fault reds for anything but a refused request or invalid input.
- **Don't** put a readable figure in Unlit Amber or Ghost Matrix; idle windows are empty cells with hidden text.
- **Don't** set words in Doto, legends in Plex, or prose in Barlow Condensed.
- **Don't** give any surface other than a pressable transport key a hard offset shadow.
- **Don't** round, restyle or re-derive a returned value; show it at the stated precision with its raw value attached.
