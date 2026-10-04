# Playwright guidelines

Scope: the browser end-to-end suite for the embedded Leptos playground (`crates/ardana-playground`, served by
`crates/ardana-server` from the `ardana` binary). It lives in `e2e/playground/` (`package.json`, `package-lock.json`,
`.npmrc`, `playwright.config.ts`, `tests/helpers.ts`, `tests/playground.spec.ts` (W6), `tests/parity.spec.ts` (W7),
`tests/first-run.spec.ts`, `tests/browser.spec.ts` (the in-tab engine), `tests/cli.spec.ts` (models the server cannot
run), `tests/design.spec.ts` (the design suite's in-tab states), `tests/reflow.spec.ts` (zoom and enlarged text),
`tests/focus.spec.ts` (focus, history, announcements, ARIA), `tests/polish.spec.ts` (touch reach, the current row, the
first load's weight), `tests/standalone.spec.ts` (the standalone build), `standalone-hosts.mjs` (its two hosts),
`fixtures/jev-share-links.json`) and runs through `cargo xtask e2e playground` (W6 R6.3-R6.7, W7 R7.1-R7.6, W2
R2.2-R2.6, W3 R3.3-R3.5) and `cargo xtask e2e standalone` (the standalone build), next to the `impeccable detect`
scans and the finish check of `cargo xtask e2e design` (Q24, R6.2, R7.7, R2.7, R3.6).
It covers npm setup inside the sandbox, config, browsers, locators, assertions, network timing, screenshots and
share-link decoding.

## Versions
- `@playwright/test` 1.63.0 — test runner, fixtures, web-first assertions; pinned exactly in `package.json` (Q20, R6.8)
- `lz-string` 1.5.0 — decodes Jev share links in tests with `decompressFromEncodedURIComponent` (Q28, R7.4, R7.8)
- `@typesafe-ai/sdk` 0.6.0 — the official TypeSafe TypeScript SDK the `snippets` case runs the TypeScript snippet with
  (R7.6, R7.8); ESM and CJS builds, no dependencies, needs Node 20+
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
- Ardana's choice: `cargo xtask e2e playground` starts the server (the release binary copied alone into
  `tmp/e2e/playground-binary`, decider-2b pulled offline, `crates/ardana-playground/dist` moved away while it runs)
  and passes `ARDANA_BASE_URL` and `ARDANA_REPO_ROOT`; the config has no `webServer` and throws without
  `ARDANA_BASE_URL`. It runs `playwright test --grep-invert "@design|@standalone"`: tests tagged `@design` belong to
  `cargo xtask e2e design`, which runs `--grep @design --reporter list --output tmp/playwright/design-results`, and
  tests tagged `@standalone` to `cargo xtask e2e standalone`, which runs `--grep @standalone --reporter list --output
  tmp/playwright/standalone-results`, so the playground suite's reports stay its own. `cargo xtask e2e standalone`
  starts `node e2e/playground/standalone-hosts.mjs` first, which prints the URLs of its two hosts (the standalone
  `dist` under `/playground/` with the isolation headers the landing sends, and at `/models.json` the file xtask
  copied the snapshot to (`tmp/playground/models.json`), read on every request and served as Cloudflare serves the
  landing's: an `ETag`, `max-age=0, must-revalidate`, a 304 for an `If-None-Match` that holds it, a 404 once the file
  is gone; and a stand-in of Hugging Face over `tmp/hf/hub`, CORS open, `Range` answered with a 206, a request naming
  a referring page refused with a 404 as huggingface.co refuses one from `*.workers.dev`), builds the playground with
  `--hub` at the stand-in (checking the build wrote no `tmp/playground/library.json` and its dist names no repository
  of the snapshot), and passes `ARDANA_BASE_URL` (the site), `ARDANA_HUB_URL`, `ARDANA_SITE_LIBRARY` (that file, which
  the cases rewrite and take away; each starts it over from the snapshot) and `ARDANA_BIN` (the release `ardana`,
  whose `ardana run` answers the tab is compared with). The R6.4 `placeholder_without_dist` case is a cargo build plus
  `ardana-server`'s `playground` test in `tmp/e2e/placeholder/target`, run by xtask before Playwright.
- `webServer`: either the config launches the binary that `cargo xtask build` produced (`command`, `url` pointing at
  `http://127.0.0.1:<port>/health`, `reuseExistingServer: false`, `timeout` covering startup, `stdout: 'pipe'`), or
  `cargo xtask e2e playground` starts the server itself and passes the base URL in; pick one per case and never both.
  `webServer.env` inherits `process.env`, so the sandbox variables reach the server. Use `url`, not the deprecated
  `port`. Cases that need their own server (R6.3 `embedded_binary`, R6.4 `placeholder_without_dist`) start it in the
  test or in xtask and stop it in `afterAll`.

## Patterns
- Screenshots, Ardana's naming: each case's final state is `tmp/screens/<case>/<project>.png`; intermediate states it
  also captures are `<state>-<project>.png` (`answers_match_api/loaded-*`, `picker_raw_errors/413-*`,
  `picker_raw_errors/unknown-type-*`). xtask checks every case's pair exists at 1280 and 390 px wide. Screenshots
  pass `animations: 'disabled'` so the bars show their final widths.
- Figures (R6.5): `expectFiguresMatch` in `tests/helpers.ts` reads every `[data-field]` element, resolves its JSON
  pointer in the response the page received, and checks `data-value` (numbers compared as numbers) and the text in
  its `data-format`; `roundDecimal` implements the display rounding independently of the Rust code (BigInt, half up
  on the decimal text).
- Exact text: `toHaveText` normalises whitespace, so the raw panels are compared with
  `toHaveJSProperty('textContent', exact)`, which retries and compares byte for byte.
- The only routed step is R6.6's unknown answer `type`: the server never returns one, so `picker_raw_errors` fetches
  the real response with `route.fetch()`, renames one answer's type and fulfils it; nothing else is routed.
- Same-origin check: trunk's loader fetches the wasm with `fetch`, so the check allows `fetch` requests for `.wasm`
  and requires every other `fetch`/`xhr` request to go to `<origin>/v1/`. A run in the tab also imports
  onnxruntime-web (`/ort/1.30.0/*.mjs`, `script` requests) and its bundle fetches its `.wasm`; `browser_run` and
  `embedded_binary` check that every request of theirs stayed on the page's origin.
- Screenshots (R6.7): every case ends with `await page.screenshot({ path: <root>/tmp/screens/<case>/<project>.png,
  fullPage: true })` using `testInfo.project.name`; the two projects together produce the 1280x800 and 390x844 pair.
  Cases with several final states name each `<state>-<project>.png` (`jev_share_links/<page>-*`, `presets/<preset>-*`);
  xtask requires at least one screenshot per project in every case directory and checks every one's width.
  Automatic `use.screenshot`/`trace` artifacts stay in `outputDir`.
- Request timing (R6.5): take the finished request with
  `page.waitForEvent('requestfinished', r => r.url().endsWith('/v1/systemone'))`, then read `request.timing()`.
  Values are milliseconds relative to `timing.startTime` and `-1` when unavailable; compare the displayed latency
  with `timing.responseEnd` (fail if it is `-1`) within 20 ms.
- Same-origin check (R6.6): collect `page.on('request', r => urls.push(r.url()))` from the first `goto` and assert every
  API URL starts with `<baseURL>/v1/`; use `request.postDataJSON()` to compare the sent body with the raw panel.
- Share links in W6 cases are written with `lz-string`'s `compressToEncodedURIComponent` (`shareHash` in
  `tests/helpers.ts`), as Jev's playground writes them, so the Rust decoder is checked against the JS encoder.
- `run(page)` in `tests/helpers.ts` presses Run and waits for the `/v1/systemone` response with `timeout: 0`, so a
  heavy request is bounded by the test's own timeout (`test.setTimeout`), not the 30 s default of `waitForResponse`.
- The model picker, the presets and Restore previous live in the sidebar, a drawer behind the "Open sidebar" button
  on the mobile project: `pickModel`, `loadPreset` and `restorePrevious` in `tests/helpers.ts` open it when it is
  closed and close it again (`inSidebar`). Never click a sidebar control directly in a case that runs on both
  projects.
- Docs share links (R7.3): `fixtures/jev-share-links.json` vendors every `console.typesafe.ai/playground#share/` link
  found on docs.typesafe.ai (17, all on cookbook pages) with its page. `jev_share_links` has one test per link: the
  editors hold the decoded `documentText` and `promptsText`, the picker the first `selectedModels` entry (the
  server's default model, decider-2b, when absent or a `jev-*` alias), and every question id gets a displayed answer. Two links name `speed_latest`, which
  Ardana does not serve: the test first checks the 404 fault naming it (Q7), then picks decider-2b and runs. The
  largest links take minutes (every row decodes a 12k-token state), hence `test.setTimeout(900_000)`.
- First run (`first_run`, R3.5): xtask starts one more `ardana serve` on an empty `ARDANA_HOME` with
  `HF_HUB_OFFLINE=1` (`Server::start_empty`) and passes it as `ARDANA_EMPTY_URL`; a run in the tab pulls no model, so
  both projects find the registry empty. The case checks `/v1/models` (nothing pulled, decider-2b the default,
  decider-0.8b alone `x_browser_default`), opens a Jev-style share link (`jev-latest`), checks the picker opens on
  decider-0.8b's "In browser" row with its note and that no readable text (`visibleText`: body text, option and
  optgroup labels, `placeholder`, `aria-label`, `title`) contains "jev", then presses Run: the answer comes from the
  tab (`decider-0.8b-v1`, "in this tab on"), every `data-value` matches its raw panel, no request goes to
  `/v1/systemone`, and the registry is still empty.
- A model the server cannot run (`run_command`, R3.3 as the user amended it, in `tests/cli.spec.ts`): on the
  decider-2b server, a share link naming decider-4b opens on its server row; Run is held (`aria-disabled`, its
  description and the banner say "This server has not pulled decider-4b: pull it with the ardana CLI"), the snippets
  show `ardana pull decider-4b` (`pull`, with what it downloads in `pull-note`) and `ardana run <name> --request -`
  whose heredoc (`ardanaCommand`) is exactly `JSON.stringify(request, null, 2)` of the request Run would send, and no
  `install` line (a local server runs ardana already); neither Run (clicked with `force`: Playwright waits on
  `aria-disabled` buttons) nor Ctrl+Enter sends anything (the Run label, `aria-busy`, an unchanged `run-status` and the
  request log), and the held Run shows no tooltip when hovered (its `::after` is `display: none`). "Show the command"
  focuses the Snippets summary with the pull in view, every other model the server has not pulled shows the same, so
  does a link naming `Decider-4B` (on decider-4b's row) and one naming `decider-4b:q8_0` (a quant the server would
  pull, which the list does not size) with and without `?autorun=1`, with no `POST /v1/systemone` from any; an "In
  browser" row shows the ardana command without the install line, after `ardana pull` for decider-0.8b (not pulled
  here) and alone for decider-2b (pulled); and decider-2b then runs on the server again.
- The standalone build (`tests/standalone.spec.ts`, `@standalone`): the page at `/playground/` GETs `/models.json`
  once on load, requests nothing under `/v1/` and nothing from another origin until a run in the tab, every other
  request under `/playground/` and none failed (`requestfailed`, or a status of 400 and over), and lists the served
  document's entries in its order (the snapshot's at first, `crates/ardana-registry/tests/data/models.json`, which
  `run_command` reads too for the models the server lacks); `standalone_browser_run` runs in a persistent profile of
  its own, as `browser_run` does, takes `ardana run decider-0.8b --request - --json` (`ARDANA_BIN`, a home of its own,
  `HF_HUB_OFFLINE=1`) as the answer to match, and expects exactly the three file URLs
  `<hub>/<org>/<repo>/resolve/<browser.commit>/<file>` from the stand-in; after a reload it sums the stand-in's
  `responseBodySize`s. The server rows hold Run behind ardana.ai's install line (`INSTALL`), and nothing goes out when
  Run is pressed. `standalone_library_update` adds an entry to the served file and sends the tab's return (the
  `focus` and `visibilitychange` events, as `pull_while_open` does), expecting the row with no reload (a value the
  page's script held survives); `standalone_library_unavailable` removes the file before the load (the `models-fault`
  alert under the picker, an empty picker with no note, Run held by "The model library is unavailable", Snippets'
  `ardana run <model>`), serves it again with an entry for a later `ardana` and one that does not read (both left
  out, the browser default's row picked, the editors kept), removes it again (the alert, the list and the pick kept,
  Run free) and serves a `schema: 2` document (the alert with the schema reason), counting the document's GETs: one
  per load and per return.
- A pull while the page is open (`pull_while_open`): on one more empty server (`ARDANA_PULL_URL`, its home in
  `ARDANA_PULL_HOME`), so the other servers' lists stay as their cases expect, the case first undoes the other
  project's pull (`ardana rm`, entries only), picks qwen3.5-0.8b's server row (the pull shown, Run held), runs the shown
  `ardana pull qwen3.5-0.8b` with the binary under test (`ARDANA_BIN` first on `PATH`, `HF_HUB_OFFLINE=1` over
  `tmp/hf`) and sees the server list it while the page still holds Run; the tab coming back lists the models again, the
  row is a "Pulled" server row, and Run's `POST /v1/systemone` answers from the server, every figure matching; removed
  again with `ardana rm`, the page shown again holds Run with the pull once more. Headless Chrome keeps every page
  visible and focused (`bringToFront` and a minimised window send nothing), so the case dispatches the events a tab
  switch sends: `focus` at the window, then `visibilitychange` at the document.
- In the tab (`browser_run`, W2): xtask starts one more server on an empty registry (`ARDANA_BROWSER_URL`), since the
  case compares the tab with `POST /v1/systemone` for decider-0.8b, which pulls and loads its GGUF there; the other
  cases keep a server holding decider-2b alone. Each project runs in a persistent Chrome profile of its own under its
  output directory, emptied first (`chromium.launchPersistentContext`): a visitor whose browser keeps the files, where
  an incognito context's Cache Storage refuses the 447 MB weights. The case picks decider-0.8b's "In browser" row
  (`pickInBrowser`, the value `browserRow(name)`), sees the banner's size line ("Run downloads decider-0.8b into this
  tab once: ..."), loads the ticket preset and runs it while a `MutationObserver` records the Run label, the banner's
  words (`#run-note-text`) and the download bar's value, and every text of the `run-status` region. The server holds the
  variant whole in `tmp/hf` (`x_browser_pulled`), so although the first run's requests wait 1 s each (CDP's
  `Network.emulateNetworkConditions` `latency`) the log holds no pull: one Downloading stage across the three files with
  the bytes rising to `x_browser` and no Starting between them, then Starting once, then "Running ... on WebGPU", and no
  request to `/v1/systemone`; the status region heard the download in eleven tenths (0% to 100%), the start, the run and
  the answer, each once; during the download Run (`Loading`) is described by the banner's words alone and the bar is
  `aria-hidden`. The pipe is narrowed (48 MB/s) only until the download is photographed (`download-<project>.png`): the
  bytes are the server's, only slower. It then checks the tab's response (its raw panel) against every displayed figure
  and its top answers (`topAnswers`: the choice, the most probable level, noul at 0.5) against the server's; that a pick
  of the model's server row the moment a row starts decoding (an in-page `MutationObserver` on the banner) stops the run
  (its status says only "Stopped, not answered": no download to resume) and releases the tab's model only once that row
  is over (`WATCH_SESSIONS` wraps onnxruntime-web's `InferenceSession.prototype.run` and `release` in the page: one
  `release`, after a `ran`, none under a run and no failed row), then that a run back on its "In browser" row fetches
  only the profile, says Starting and answers alike (the model loaded again from the kept files); after a reload, that
  the next run, its requests waiting 600 ms each, fetched only the profile, under 1 MB, showed no size line, no pull (in
  the banner or the status region) and no download; in a page whose init script deletes `Navigator.prototype.gpu`, that
  it runs on WASM to the same top answers; and that a question the server refuses with 422 shows in the tab the server's
  body byte for byte. It logs the bytes, the top answers and the 422 body.
- An insecure page (`insecure_origin`): a Chrome of its own launched with `--host-resolver-rules=MAP insecure.test
  127.0.0.1` opens the browser server as `http://insecure.test:<port>`, which is no secure context (no `caches`, no
  WebGPU, no cross-origin isolation). Its model note and size line say a page without HTTPS keeps no files, the run
  answers on WASM (it does not hang), every figure matches the tab's response, nothing says the files are kept, and
  every request stays on that origin. A run stopped mid-download says the next run starts the download over, and no
  request carries a `Range` (the page keeps nothing to resume from). The test's own requests (`page.request`) go to
  `127.0.0.1`: only the browser resolves the mapped name.
- Stopping a run in the tab (`browser_stop`, on the empty server, a fresh context): an autorun link onto decider-0.8b's
  "In browser" row asks for no `/v1/browser/` file before the tap, shows the size line in the main column (in view at
  both widths) as Run's description and leaves Run idle; after the tap, with the pipe narrowed to 16 MB/s, Stop in the
  banner cancels the weights' request (`requestfailed` with `net::ERR_ABORTED`), starts no other, says "Stopped, not
  answered. The next run resumes the download." in the status, puts Run back with the focus on it and lands no
  answer; picking decider-2b's server row mid-download does the same.
- Focus under the sticky banner (`banner_focus`, WCAG 2.4.11, on the decider-2b server): a server run's answers stay on
  the page, the last question's builder open, while decider-0.8b downloads into the tab at 2 MB/s (`throttle` in
  `tests/helpers.ts`, which `browser.spec.ts` shares); Shift+Tab from Add question and Tab from the state editor, 24
  stops each (`walkFocus`, as `banner_reflow` walks), focus only what shows whole below the top bar's and the banner's
  bottom, or, taller than the room the page scrolls it into (below them and the 12px under them), fills that room or,
  a text field, shows its caret's part (controls in those bars, the sidebar and the skip link aside), measured a frame
  after each key, at the
  project's viewport and on the phone project also at 320x640 and 430x932, each viewport but 430x932 with 200% text too
  (`html { font-size: 200% }`); the case then stops the download. Before the "In browser" pick, the page has asked
  for neither the engine module (`/engine/ardana-engine*`) nor onnxruntime-web; the pick imports both.
- What stays on screen in flight (`banner_reflow`, on the decider-2b server, `inFlight` in `tests/helpers.ts`: a
  server run's answers, then decider-0.8b downloading at 2 MB/s): at 320x256 (1280x1024 at 400% zoom), 320x640,
  844x390 and 932x430 with 200% text, and 320x256 with 200% text, the page scrolled to its questions, the top bar's and
  the banner's bottoms hold at most a third of the viewport; Run, then Tab, focuses Stop, whole in the viewport and
  under the pointer; and `walkFocus` (Shift+Tab from Add question, Tab from the state editor, 24 stops each, the last
  builder open) meets no stop that is not clear: whole when it fits the room, else filling it, or for a text field
  taller than the room showing its caret's part. Run is located by `#run-key` in flight, where its name is Loading.
  Screenshots at those sizes are `<project>-<w>x<h>[-text200].png` (`viewShot`, which xtask's width check leaves out).
- Enlarged text on phones (`text_reflow`): with 200% text at ten widths from 280 to 480 px, on the curl snippet and on
  an "In browser" row's ardana command, no box (outside the sidebar and visually hidden text) ends past the right edge,
  the page scrolls 0 px sideways, and the snippet's copy key, focused afresh, is whole in the viewport.
- Tooltips in flight (`term_tooltips`): at 390, 430 and 1280 px and 390 px with 200% text, Shift+Tab from Add question
  and the arrow keys along each fact row; each focused term's tooltip, its `::after` box inside the transparent border,
  covers none of the banner.
- The skip link and Restore previous (`skip_link`): a share link opened by `location.hash`, then edited; Enter on the
  skip link focuses `#content` with `location.href` and `history.length` unchanged, and Back returns to the page before
  the link with the edits kept; another link over the edits, with an "In browser" row picked, offers Restore previous,
  which brings back the editors and the row; a link loaded twice over the edits still offers them; under the open
  drawer at 390 px, 16 Tabs never reach the skip link, which Shift+Tab from the opener reaches once the drawer is
  closed and hidden.
- The Share popover (`share_popover`): Tab past its copy key closes it; a click on its note sends the focus nowhere and
  keeps it open; Escape then closes it and focuses Share.
- Live regions (`announcements`): a `MutationObserver` records every text each status region takes; the same refused
  request run twice records `['', 'HTTP 422, not answered']` the second time, and the same preset loaded twice says
  "Loaded the Ticket routing preset" twice.
- A run's focus (`run_focus`): on the phone project, Ctrl+Enter from the state editor, a refused run and a run tapped
  in a `hasTouch` context land the focus on the first question's id or the fault's title, whole under the bars once
  the page has scrolled; on the desktop project it stays in the editor or on Run.
- ARIA relationships (`aria_relationships`): in every state (empty, a link loaded, the popover open and closed,
  answers, a builder open, a model the server has not pulled, a run in flight, stopped), every id an `aria-controls`,
  `aria-describedby`, `aria-labelledby`, `aria-owns`, `aria-details` or `for` names is on the page.
- Fact rows and names (`term_stops`): Tab from the state editor to Add question meets one fact term per question;
  Right, End, Home and Left move along the row; Chrome's accessibility tree (CDP `Accessibility.getFullAXTree`, the
  last node marked `focused`, as the document is too) names a focused term by its word and a focused Run, its tooltip
  showing, "Run", after a run in the tab and after a 422 there.
- Focus after the page's own changes (`page_focus`): Add option and Add level focus the new row's first field; a
  `focusin` log shows Stop, focused as the download ends, handing the focus to Run.
- Touch reach (`touch_targets`): under a mouse, a few small controls keep their drawn heights and no `::before`; in a
  `hasTouch` context of the project's size (`pointer: coarse`) every visible control of the page, the sidebar (the
  drawer opened by a tap on a phone), the Share popover, the banner with "Show the command" and the banner in flight
  with Stop takes `document.elementFromPoint` at the eight points 21.5 px from its centre.
- The current row (`current_row`): `page.emulateMedia` in light, dark and `forcedColors: 'active'`; the current "On
  this page" row is at 500 and the others at 400.
- The first load's weight (`page_weight`): with the sidebar's foot and the tooltips of the sidebar's key and of Run
  on screen, no `/fonts/onest-math-*` or `/fonts/onest-symbols-*` request; the brand file's first path is in the DOM
  once, both logos `use` its symbol, and the visible one keeps the lockup's proportions.
- Recovery (`browser_recover`, on the empty server, a fresh context): CDP `offline` mid-download fails the run with the
  tab's own words ("Not answered in this tab." and one cause with its next step, never the browser's raw text, which is
  the raw exchange's); online again with onnxruntime-web's files blocked (CDP `Network.setBlockedURLs`), the run says
  the runtime did not load; unblocked, Run imports the build again and answers, without a reload. On a page of its
  own with the engine module blocked, the run says "Ardana's engine did not load into this tab. Run again once the
  connection is back." and downloads no file; unblocked, Run imports the module again (`?retry=<n>`) and answers. The
  run after onnxruntime-web's files come back answers on WebGPU, its build asked for again under a new query; and on a
  page of its own whose WebGPU build cannot fetch its WASM module once (CDP `Network.setBlockedURLs` on
  `ort-wasm-simd-threaded.asyncify.wasm`, the HTTP cache off), that run answers on WASM and the next on WebGPU, having
  imported `ort.webgpu.bundle.min.mjs?retry=1`.
- The server's pull (`browser_pull`, on one more server over an empty Hub cache of its own, `ARDANA_UNCACHED_URL`):
  `/v1/models` marks no browser variant pulled, and the first run in the tab, every request waiting 1.5 s, says
  "Pulling the browser files of decider-0.8b (467.7 MB) on the server" with the label "Pulling" and Stop, the status
  region hears it once; the pull fails offline, and the run says "The server could not provide the browser files of
  decider-0.8b. Try again later."
- Resuming (`browser_resume`, on the browser server, a persistent profile): the page runs behind a proxy the test starts
  in front of the server (Node's `http`, every request and response forwarded as they are), which cuts the weights'
  responses in flight on demand, as a dropped connection does: Chrome's CDP `offline` leaves a response under way
  alone. The download is cut about 150 MB in (the fault: "... Run again once the connection is back to resume it."),
  stopped about 280 MB in (the status: "... The next run resumes the download."), and ended by a reload about 380 MB
  in; each next run asks for the weights with `Range: bytes=<had>-` from further on and gets the rest (206 with its
  `Content-Range`), the last takes exactly the rest of the file, the bytes received over the four runs (CDP
  `Network.dataReceived`) stay within 48 MiB of the file's length, the answers are the server's top answers, and Cache
  Storage then holds each file once, whole, with none of its parts.
- Embedded runtime (`embedded_binary`, R2.6): the four vendored onnxruntime-web files are served byte for byte from
  `/ort/1.30.0/`, every response carries the Q13 headers and no CORS header, the page is `crossOriginIsolated`, and
  picking an "In browser" row imports the WebGPU bundle from the page's origin, whose `env.versions.web` is `1.30.0`
  (read with `page.evaluate` of a string: a function's `import()` would be compiled to `require`).
- Snippets (R7.6): the `snippets` case writes each snippet to `tmp/playwright/snippets/<project>/` and executes it
  there: curl with `bash` (its body must equal the request the page sent, its output the response the page
  received), Python with `tmp/py/sdk/bin/python` and `TYPESAFE_BASE_URL`, TypeScript with the running Node
  (`decide.mts`, type stripping) through a `node_modules` symlink to `e2e/playground/node_modules`. Both SDKs get a
  placeholder `TYPESAFE_API_KEY`; the SDK outputs must equal every displayed figure except Ardana's `x_` extras,
  which the SDK models drop. With decider-0.8b's "In browser" row picked (R3.4 as amended) the language toggle gives
  way to the ardana commands, without the install line on this local server: `ardana pull decider-0.8b`, which runs as
  `pull.sh`, then `ardana run`, as `ardana.sh`, each with `bash` and the binary under test first on `PATH`
  (`ARDANA_BIN`, the copy xtask serves), in a home of their own with `HF_HUB_OFFLINE=1`: the pull takes decider-0.8b's
  GGUF from `tmp/hf` and the run titles each answer with its question. Picking decider-2b brings curl, Python and
  TypeScript back at the page origin.
- Recovery and announcements: `stale_and_restore` checks the `changed` tag, the "From last run · inputs changed"
  tags (only the edited question when one spec changes, every question when the state changes), unchanged
  `data-value`s, the question's `stale` class, Restore previous after a preset, and on the mobile project that the
  first question is in view after Run. `answers_match_api` checks Run keeps focus (the first question's id takes it on
  the phone project) and the `run-status` text, `picker_raw_errors` the 413 status text and the 422 issue marked on its
  question block, `builder_sync` focus after rename and removals, the type round trip, `notice` and the Run reason (the
  `#run-note` banner). Use `toBeFocused`, `toHaveAccessibleDescription` and `toBeInViewport` for these.
- Radio groups (the type toggle, the snippet language): the native radio covers its key, so `check()` and
  `click()` hit the real input; arrow keys move the selection (`builder_sync` switches noul to choice by keyboard).
- Share links (R7.3, R7.4): take the text after `#share/`, run `decompressFromEncodedURIComponent`, and assert the
  result is a non-empty string before `JSON.parse` (the 1.5.0 function returns `null` for `""` and does not throw on
  garbage). Check `apiVersion`, `documentText`, `promptsText`, `selectedModels`. `lz-string` 1.5.0 is CommonJS
  (`main: libs/lz-string.js`, typings in `typings/lz-string.d.ts`).
- Error states: drive real 422 (invalid share payload, R6.2's `/?autorun=1#share/<invalid>`) and 413 (oversized state)
  through the server and assert the rendered `detail` with `toHaveText`.

## impeccable detect alongside (Q24)
- `cargo xtask e2e design` builds the share links with `lz-str` (the ticket fixture, and the same with a one-option
  `department` choice for the 422 state), warms decider-2b with one request, writes each JSON report to
  `tmp/evals/design/<state>-<desktop|mobile>.json`, and first checks R6.1's context (doctor, PRODUCT.md, DESIGN.md,
  the surface brief's Operate mode and six contract blocks, `.impeccable/config.json`, the hook in
  `.claude/settings.local.json`). Doctor's `mention`-severity findings are printed, not failed: the sidecar-stale
  one compares file mtimes, which a checkout reorders.
- `cargo xtask e2e design` runs `$IMPECCABLE_BIN detect --json --viewport 1280x800 <url>` and `--viewport 390x844` on
  `/`, `/#share/<ticket>`, `/?autorun=1#share/<ticket>`, `/?autorun=1#share/<invalid>` and `run-command`
  (`/#share/<ticket naming decider-4b>`: a model the server has not pulled) of a server holding decider-2b; exit 0 is
  clean, 1 means a target was not scanned, 2 means primary findings. Treat any non-zero exit as a failure.
- Its "finish" check (R7.7) then wants a critique record under `.impeccable/critique/` (any file but `ignore.md`),
  `docs/design/audit.md` with the line `P0: 0 · P1: 0`, every scan of this run clean, and `.impeccable/config.json`
  `hook.enabled` true.
- Scan URLs, not files: Leptos markup lives in `.rs` `view!` macros and the CSR shell is empty, so only the rendered
  page shows real findings. Use the same viewports and URLs as the Playwright projects so screenshots and findings match.
- The in-tab states `browser-download` and `browser-results` (R2.7) cannot be reached by a URL: detect waits for the
  network to go idle, which a download in progress never is, and a run in the tab answers after it. The `@design`
  test (`browser_states`) runs decider-0.8b's browser variant on the design server at each viewport and freezes the
  real page twice: inside the mutation that moves the download bar past a quarter, and once the answers are drawn. A
  frozen page is the cloned document with its form state written into the markup, no script or link, and the
  stylesheets as served inline with their fonts as data URLs (a `file://` page cannot load the server's
  CORP-same-origin assets; `cssRules` would lose shorthands that use `var()`). xtask writes them to
  `tmp/evals/design/<state>-<project>.html` and scans each by `file://` URL at its viewport. The download state holds a
  run in flight, so it shows DESIGN.md's busy caret; `.impeccable/config.json` waives `blinking-cursor` for those two
  files only, with that reason.
- The standalone build's `library-unavailable` state (R6.8) is a URL state of another build: `cargo xtask e2e design`
  starts the standalone suite's hosts (`standalone-hosts.mjs`), builds the standalone playground with `--hub` at the
  stand-in, removes the file the site serves at `/models.json`, and scans `<site>/playground/` at both viewports. The
  404 settles before detect's network idle, so the page shows what a tab without its library shows: the `models-fault`
  alert under an empty picker and Run held by "The model library is unavailable".

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
- https://playwright.dev/docs/api/class-browsertype#browser-type-launch-persistent-context — a persistent profile
- https://playwright.dev/docs/api/class-cdpsession — `Network.emulateNetworkConditions` and `Network.setBlockedURLs`
  through a CDP session
- https://playwright.dev/docs/api/class-browsertype#browser-type-launch — `args` (`--host-resolver-rules`) for a
  browser of the case's own
- https://playwright.dev/docs/test-annotations#tag-tests — `{ tag: '@design' }`, `--grep` and `--grep-invert`
- https://playwright.dev/docs/intro — supported Node versions (22, 24, 26) and macOS requirements
- https://playwright.dev/docs/release-notes — 1.63 changes and bundled browser versions
- https://docs.npmjs.com/cli/v11/commands/npm-ci — lockfile-exact, read-only installs
- https://docs.npmjs.com/cli/v11/using-npm/config — `cache`, `npm_config_*` env mapping, `save-exact`, `logs-dir`
- https://github.com/pieroxy/lz-string/blob/1.5.0/libs/lz-string.js — `decompressFromEncodedURIComponent` behavior, exports
- https://github.com/pieroxy/lz-string/blob/1.5.0/package.json — 1.5.0 `main` and `typings`
- https://www.npmjs.com/package/@typesafe-ai/sdk — `TypeSafeClient({ baseURL, apiKey, timeout })`, `systemOne`,
  `TYPESAFE_API_KEY` and `TYPESAFE_BASE_URL`
- https://docs.typesafe.ai/sitemap.xml — the docs pages the share links were collected from
- https://nodejs.org/api/typescript.html — Node's type stripping runs `.mts` files
- https://nodejs.org/en/about/previous-releases — Node 24 LTS status
