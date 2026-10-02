# Brand assets

Copies of the Ardana logo kit and of ardana.ai's type, so the playground wears the site's brand offline. Regenerate
them from the sources below; never edit them by hand.

| File | Source |
| --- | --- |
| `ardana-logo.svg` | The kit's tight horizontal lockup, `ardana-landing/brand/src/logo-tight.svg`, with its fill set to `currentColor` and its title, role and size dropped: the drawing of the landing's `Logo.svelte` (both come from `brand/src/build.py`). `ui::logo` inlines it. |
| `favicon.svg`, `favicon.ico` | `ardana-landing/static/`, as the kit's `build.py` writes them; the SVG follows the browser's light or dark scheme. trunk copies both to the root of `dist/`. |
| `../fonts/*.woff2` | `@fontsource-variable/onest` 5.3.1 and `@fontsource-variable/geist-mono` 5.3.0 (the `wght-normal` file of every subset), the packages ardana.ai loads; their licences are `../fonts/OFL-*.txt` (SIL OFL 1.1). `styles/fonts.css` declares them with fontsource's `unicode-range`s. |
| `crates/ardana-server/placeholder/fonts/` | The latin files of both faces and their licences, copied from `../fonts/` for the page a binary serves when it is built without the playground; `placeholder.css` declares them. |
