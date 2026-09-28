# Playwright guidelines

Scope: the browser end-to-end suite for the embedded Leptos playground (`crates/ardana-playground`, served by
`crates/ardana-server` from the `ardana` binary). It lives in `e2e/playground/` (`package.json`, `package-lock.json`,
`playwright.config.ts`, `tests/`, `fixtures/jev-share-links.json`) and runs through `cargo xtask e2e playground`
(W6 R6.3-R6.7, W7 R7.1-R7.6), next to the `impeccable detect` URL scans of `cargo xtask e2e design` (Q24, R6.2).
It covers npm setup inside the sandbox, config, browsers, locators, assertions, network timing, screenshots and
share-link decoding.

## Versions
- `@playwright/test` 1.63.0 — test runner, fixtures, web-first assertions; pinned exactly in `package.json` (Q20, R6.8)
- `lz-string` 1.5.0 — decodes Jev share links in tests with `decompressFromEncodedURIComponent` (Q28, R7.4, R7.8)
- `node` 24 — the installed Node on the dev machine (24.18.1, LTS "Krypton"); Playwright 1.63 supports Node 22, 24 and 26

## Rules
- Pin every dependency with an exact version (no `^`/`~`) in `e2e/playground/package.json` and commit
  `package-lock.json`; `.npmrc` sets `save-exact=true` so later additions stay exact.
- Install only with `npm ci` from `e2e/playground/`, after `eval "$(cargo xtask env)"`, so `npm_config_cache` points at
  `tmp/cache/npm` (npm's logs default to `_logs` inside that cache). `npm ci` fails on a lock mismatch instead of
  rewriting it; fix the lock deliberately, never by running `npm install` in a script. `cargo xtask fetch` drives this.
- Keep `e2e/playground/node_modules` gitignored; `rm -rf tmp e2e/playground/node_modules` must fully reset the suite.
- Drive the installed Google Chrome with `use.channel: 'chrome'` in every project. Never run `npx playwright install`
  (it downloads into `~/Library/Caches/ms-playwright` by default) and never `npx playwright install chrome` (it
  installs branded Chrome at the OS-wide location, replacing the user's Chrome).
- Always run with `PLAYWRIGHT_BROWSERS_PATH=tmp/ms-playwright` from the sandbox env, so anything that would fetch a
  browser lands in `tmp/` and the home guard stays clean; browser profiles go to the OS temp dir, which is `tmp/sys`.
- Run Playwright only through `cargo xtask e2e playground` (guarded) or, when iterating, `npx playwright test` in a
  shell that has evaluated `cargo xtask env`. Never from a shell with the real `HOME` caches.
- Run end-to-end requirements against the real `ardana` server and a real model (decider-2b Q4_K_M from `tmp/hf`).
  Use `page.route` / `route.fulfill` only in clearly separate unit-ish specs (e.g. a stub `422` body to exercise
  rendering) and name them so; never let a routed spec satisfy an R6.x/R7.x case.
- Bind the server to `127.0.0.1` on an explicit port and point `use.baseURL` at `http://127.0.0.1:<port>`; tests call
  `page.goto('/')`, `page.goto('/#share/...')`, never a hard-coded origin.
- Locate by role first (`getByRole('button', { name: 'Run' })`), then label/text, then `getByTestId` for elements
  without an accessible name (bars, result rows). Keep the default `data-testid` attribute; do not use CSS/XPath
  selectors for playground structure.
- Use only auto-retrying, awaited assertions on the page: `await expect(locator).toHaveText(...)`,
  `toHaveAttribute('data-value', ...)`, `toHaveCount`, `toBeVisible`, `toHaveURL`. Never
  `expect(await locator.textContent())` and never `page.waitForTimeout`.
- Compare displayed values with the API response the page itself received: capture it with
  `page.waitForResponse(r => r.url().endsWith('/v1/systemone'))` started before the click, read `await response.json()`,
  then assert each field's `data-value` equals the raw JSON value (one shared serializer for numbers/strings) and its
  text equals the formatted value (percent with one decimal for probabilities, two decimals otherwise; R6.5).
- Keep `workers: 1` and `fullyParallel: false`: the server decodes one prompt at a time from a FIFO queue, so parallel
  tests skew latency checks and can hit 503. Raise `timeout` and `expect.timeout` for model-backed cases, and warm the
  model with one request before the latency assertion so lazy load is not measured.
- Keep tests isolated: each test opens its own page state (fresh `page.goto`), and the share round trip (R7.4) opens
  the link in a new page from a fresh context, not by reloading the same page.
- Set `forbidOnly: true` and `retries: 0`; a flaky end-to-end case is a bug to fix, not to retry.

## Config (`e2e/playground/playwright.config.ts`)
- Resolve every path from the repo root (`path.resolve(__dirname, '../..')`) so outputs never land in
  `e2e/playground/`.
- `outputDir: <root>/tmp/playwright/results`. The `--last-failed` state defaults to `<outputDir>/.last-run.json`, so it
  lands at `tmp/playwright/results/.last-run.json`; do not set `PLAYWRIGHT_LAST_RUN_OUTPUT_FILE` elsewhere.
- `reporter: [['list'], ['html', { outputFolder: <root>/tmp/playwright/report, open: 'never' }],
  ['json', { outputFile: <root>/tmp/playwright/results.json }]]`. The HTML report folder must be a sibling of
  `outputDir`, never inside it or around it: the HTML reporter clears its folder and reports a configuration error on
  overlap.
- `projects`: two Chrome projects differing only in viewport, e.g. `{ name: 'desktop', use: { channel: 'chrome',
  viewport: { width: 1280, height: 800 } } }` and `{ name: 'mobile', use: { channel: 'chrome', viewport: { width: 390,
  height: 844 } } }`. Do not spread `devices['iPhone ...']` descriptors: they switch the browser type away from Chrome.
- `webServer`: either the config launches the binary that `cargo xtask build` produced (`command`, `url` pointing at
  `http://127.0.0.1:<port>/health`, `reuseExistingServer: false`, `timeout` covering startup, `stdout: 'pipe'`), or
  `cargo xtask e2e playground` starts the server itself and passes the base URL in; pick one per case and never both.
  `webServer.env` inherits `process.env`, so the sandbox variables reach the server. Use `url`, not the deprecated
  `port`. Cases that need their own server (R6.3 `embedded_binary`, R6.4 `placeholder_without_dist`) start it in the
  test or in xtask and stop it in `afterAll`.

## Patterns
- Screenshots (R6.7): every case ends with `await page.screenshot({ path: <root>/tmp/screens/<case>/<project>.png,
  fullPage: true })` using `testInfo.project.name`; the two projects together produce the 1280x800 and 390x844 pair.
  Automatic `use.screenshot`/`trace` artifacts stay in `outputDir`.
- Request timing (R6.5): take the finished request with
  `page.waitForEvent('requestfinished', r => r.url().endsWith('/v1/systemone'))`, then read `request.timing()`.
  Values are milliseconds relative to `timing.startTime` and `-1` when unavailable; compare the displayed latency
  with `timing.responseEnd` (fail if it is `-1`) within 20 ms.
- Same-origin check (R6.6): collect `page.on('request', r => urls.push(r.url()))` from the first `goto` and assert every
  API URL starts with `<baseURL>/v1/`; use `request.postDataJSON()` to compare the sent body with the raw panel.
- Share links (R7.3, R7.4): take the text after `#share/`, run `decompressFromEncodedURIComponent`, and assert the
  result is a non-empty string before `JSON.parse` (the 1.5.0 function returns `null` for `""` and does not throw on
  garbage). Check `apiVersion`, `documentText`, `promptsText`, `selectedModels`. `lz-string` 1.5.0 is CommonJS
  (`main: libs/lz-string.js`, typings in `typings/lz-string.d.ts`).
- Error states: drive real 422 (invalid share payload, R6.2's `/?autorun=1#share/<invalid>`) and 413 (oversized state)
  through the server and assert the rendered `detail` with `toHaveText`.

## impeccable detect alongside (Q24)
- `cargo xtask e2e design` runs `$IMPECCABLE_BIN detect --json --viewport 1280x800 <url>` and `--viewport 390x844` on
  `/`, `/#share/<ticket>`, `/?autorun=1#share/<ticket>` and `/?autorun=1#share/<invalid>` of a running server; exit 0
  is clean, 1 means a target was not scanned, 2 means primary findings. Treat any non-zero exit as a failure.
- Scan URLs, not files: Leptos markup lives in `.rs` `view!` macros and the CSR shell is empty, so only the rendered
  page shows real findings. Use the same viewports and URLs as the Playwright projects so screenshots and findings match.

> Plan note: Playwright's HTML reporter errors when its `outputFolder` overlaps `outputDir`, so R6.7's layout under `tmp/playwright/` needs separate `results/` and `report/` folders.

## Sources
- https://playwright.dev/docs/test-configuration — testDir, outputDir, reporter, projects, webServer, use options
- https://playwright.dev/docs/api/class-testconfig — outputDir default, preserveOutput, forbidOnly, workers, retries
- https://playwright.dev/docs/test-reporters — list/html/json reporters, `outputFolder`, `open`, `outputFile`
- https://github.com/microsoft/playwright/blob/main/packages/playwright/src/reporters/html.ts — HTML report folder clash check
- https://playwright.dev/docs/test-cli — `--project`, `--last-failed`, default `<outputDir>/.last-run.json`
- https://playwright.dev/docs/test-webserver — webServer options, ready status codes, env inheritance, `port` deprecated
- https://playwright.dev/docs/browsers — `channel: 'chrome'`, branded install location, `PLAYWRIGHT_BROWSERS_PATH`
- https://playwright.dev/docs/test-use-options — baseURL, viewport, channel, screenshot/trace, testIdAttribute
- https://playwright.dev/docs/test-projects — projects with per-project `use`, `testInfo.project.name`
- https://playwright.dev/docs/best-practices — user-facing locators, web-first assertions, isolation
- https://playwright.dev/docs/locators — locator priority, `data-testid` default, strictness
- https://playwright.dev/docs/test-assertions — auto-retrying matchers, `toHaveAttribute`, expect timeout
- https://playwright.dev/docs/network — `page.route`, `route.fulfill`, network events, `waitForResponse`
- https://playwright.dev/docs/api/class-request — `request.timing()` fields and units, `postDataJSON`
- https://playwright.dev/docs/api/class-page — request/response/requestfinished events, `page.screenshot` options
- https://playwright.dev/docs/api/class-testinfo — `testInfo.project`, `outputPath`, `attach`
- https://playwright.dev/docs/intro — supported Node versions (22, 24, 26) and macOS requirements
- https://playwright.dev/docs/release-notes — 1.63 changes and bundled browser versions
- https://docs.npmjs.com/cli/v11/commands/npm-ci — lockfile-exact, read-only installs
- https://docs.npmjs.com/cli/v11/using-npm/config — `cache`, `npm_config_*` env mapping, `save-exact`, `logs-dir`
- https://github.com/pieroxy/lz-string/blob/1.5.0/libs/lz-string.js — `decompressFromEncodedURIComponent` behavior, exports
- https://github.com/pieroxy/lz-string/blob/1.5.0/package.json — 1.5.0 `main` and `typings`
- https://nodejs.org/en/about/previous-releases — Node 24 LTS status
