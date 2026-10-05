// W2: decider:0.8b's "In browser" row runs the ticket preset in the tab, on the browser variant the server pulls once
// from tmp/hf (`HF_HUB_OFFLINE=1`) and serves from /v1/browser/decider:0.8b/*. `browser_run` uses a server of its own
// (`ARDANA_BROWSER_URL`, an empty registry), since comparing the tab with `POST /v1/systemone` pulls and loads
// decider:0.8b there; it runs in a persistent profile of its own, emptied first: a visitor whose browser keeps the files
// across visits (an incognito context's Cache Storage cannot hold the 447 MB weights). `insecure_origin` runs the same
// row on a page that is no secure context, which has no Cache Storage. `browser_stop` and `browser_recover` run in fresh
// contexts on the empty server (`ARDANA_EMPTY_URL`), whose registry their runs in the tab leave empty: a first run in
// the tab waits for a tap (autorun included), can be stopped, and recovers from a dropped connection without a reload.
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import {
  type BrowserContext,
  chromium,
  expect,
  type Page,
  type Response as PlaywrightResponse,
  test,
} from '@playwright/test';
import {
  IN_MEMORY,
  decimalSize,
  expectFiguresMatch,
  fixture,
  isSystemOne,
  loadPreset,
  pickInBrowser,
  pickModel,
  picker,
  runKey,
  screenshot,
  shareHash,
  throttle,
  topAnswers,
  visibleText,
  walkFocus,
  type Json,
  type ModelInfo,
  type Request,
} from './helpers';

const MODEL = 'decider:0.8b';
const base = process.env.ARDANA_BROWSER_URL!;
const empty = process.env.ARDANA_EMPTY_URL!;

type Response = { model: string; answers: Record<string, Record<string, Json>>; usage: Record<string, number> };

/**
 * Records, every time the page changes, what the Run key and the banner's words say and the download bar's value
 * (`seen`), and every text the run's status region takes (`said`).
 */
async function watch(page: Page) {
  await page.evaluate(() => {
    const seen: string[] = [];
    const said: string[] = [];
    Object.assign(window, { seen, said });
    const record = () => {
      const key = document.querySelector('[data-testid="run-label"]')?.textContent ?? '';
      const note = document.querySelector('#run-note-text')?.textContent ?? '';
      const bar = document.querySelector<HTMLProgressElement>('#run-note progress');
      const entry = `${key} | ${note}${bar ? ` | ${bar.value}/${bar.max}` : ''}`;
      if (seen[seen.length - 1] !== entry) {
        seen.push(entry);
      }
      const status = document.querySelector('[data-testid="run-status"]')?.textContent ?? '';
      if (said[said.length - 1] !== status) {
        said.push(status);
      }
    };
    new MutationObserver(record).observe(document.body, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
    });
  });
}

const seen = (page: Page) => page.evaluate(() => (window as unknown as { seen: string[] }).seen);
const said = (page: Page) => page.evaluate(() => (window as unknown as { said: string[] }).said);

/** Presses Run and waits until the run is over: the key has been busy and says Run again. */
async function runInTab(page: Page) {
  await watch(page);
  await runKey(page).click();
  await expect
    .poll(
      async () => {
        const entries = await seen(page);
        return entries.some((e) => !e.startsWith('Run |')) && entries[entries.length - 1].startsWith('Run |');
      },
      { timeout: 600_000 },
    )
    .toBe(true);
}

/** The page's own answer: the exact body its raw panel shows. */
async function tabResponse(page: Page): Promise<Response> {
  return JSON.parse((await page.getByTestId('raw-response').textContent())!) as Response;
}

/** The size line a first run in this tab shows before the tap, where this browser keeps the files, and its memory. */
const firstDownload = (size: number) =>
  `Run downloads ${MODEL} into this tab once: ${decimalSize(size)}, kept by this browser. ${IN_MEMORY}.`;

/** Run's description while this tab downloads `size` bytes: the banner's words, never the bar's raw value. */
const downloading = (size: number) =>
  new RegExp(`^Downloading decider:0\\.8b into this tab: [\\d.]+ [KMG]?B of ${decimalSize(size).replace('.', '\\.')}$`);

/** What the run status says once a run in a tab that keeps the files is stopped. */
const STOPPED = 'Stopped, not answered. The next run resumes the download.';

/** The Run key while this tab downloads or starts a model: its label then says Loading. */
const loading = (page: Page) => page.getByRole('button', { name: 'Loading', exact: true });

/**
 * Records in the page every row and release of onnxruntime-web's sessions, in `sessionEvents`: `run`, then `ran` or
 * `run failed: <why>`, for each row, and `release`, or `release under a run` when a row of that session is still
 * decoding. It wraps the methods of the WebGPU build the page imported (an import of the same URL is the same module),
 * so every session, the loaded one too, reports.
 */
const WATCH_SESSIONS = `import('/ort/1.30.0/ort.webgpu.bundle.min.mjs').then((ort) => {
  const session = ort.InferenceSession.prototype;
  const { run, release } = session;
  const decoding = new WeakMap();
  const events = (window.sessionEvents = []);
  session.run = async function (...args) {
    decoding.set(this, (decoding.get(this) ?? 0) + 1);
    events.push('run');
    try {
      const outputs = await run.apply(this, args);
      events.push('ran');
      return outputs;
    } catch (err) {
      events.push('run failed: ' + (err?.message ?? err));
      throw err;
    } finally {
      decoding.set(this, decoding.get(this) - 1);
    }
  };
  session.release = function () {
    events.push(decoding.get(this) ? 'release under a run' : 'release');
    return release.apply(this);
  };
})`;
const sessionEvents = (page: Page) =>
  page.evaluate(() => (window as unknown as { sessionEvents: string[] }).sessionEvents);

/** Arms the page to pick `name`'s server row the moment the banner says a row decodes, in that same turn. */
const pickOnceRunning = (page: Page, name: string) =>
  page.evaluate((name) => {
    const observer = new MutationObserver(() => {
      if (document.querySelector('#run-note-text')?.textContent?.startsWith('Running ')) {
        observer.disconnect();
        const picker = document.querySelector<HTMLSelectElement>('#model')!;
        picker.value = name;
        picker.dispatchEvent(new Event('input', { bubbles: true }));
        picker.dispatchEvent(new Event('change', { bubbles: true }));
      }
    });
    observer.observe(document.body, { subtree: true, childList: true, characterData: true });
  }, name);

/** The bytes of `MODEL`'s browser files (`x_browser`), from `/v1/models` of the server at `origin`. */
async function browserSize(page: Page, origin: string): Promise<number> {
  const listed = (await (await page.request.get(`${origin}/v1/models`)).json()) as { models: ModelInfo[] };
  return listed.models.find((m) => m.name === MODEL)!.x_browser!;
}

test('browser_run', async ({}, testInfo) => {
  test.setTimeout(900_000);
  expect(base, 'xtask passes the server of this case').toBeTruthy();
  const origin = new URL(base).origin;
  const profile = testInfo.outputPath('profile');
  fs.rmSync(profile, { recursive: true, force: true });
  const context: BrowserContext = await chromium.launchPersistentContext(profile, {
    channel: 'chrome',
    viewport: testInfo.project.use.viewport,
    baseURL: base,
  });
  try {
    // The server's own answer to the same request, from decider:0.8b's GGUF (which this pulls and lists).
    const ticket = fixture('ticket.json');
    const request: Request = { ...ticket, model: MODEL };
    const served = await context.request.post('/v1/systemone', { data: request });
    expect(served.status()).toBe(200);
    const server = (await served.json()) as Response;
    const models = (await (await context.request.get('/v1/models')).json()) as { models: ModelInfo[] };
    const browserModels = models.models.filter((m) => m.x_browser !== undefined);
    expect(browserModels.map((m) => m.name).sort()).toEqual(['decider:0.8b', 'decider:2b', 'qwen3.5:0.8b']);
    const size = browserModels.find((m) => m.name === MODEL)!.x_browser!;
    // The server holds decider:0.8b's browser files whole in its hub cache (`tmp/hf`): a request for them pulls
    // nothing.
    expect(browserModels.find((m) => m.name === MODEL)!.x_browser_pulled).toBe(true);

    const page = context.pages()[0] ?? (await context.newPage());
    const requests: string[] = [];
    page.on('request', (r) => requests.push(r.url()));
    await page.goto('/');

    // The picker lists every browser variant in its own group; the pick says where Run answers and what it downloads,
    // and before the first tap the banner says it too, at every width.
    await expect(picker(page).locator('optgroup[label="In browser · runs in this tab"] option')).toHaveText(
      browserModels.map((m) => `${m.name} · ${decimalSize(m.x_browser!)}`),
    );
    await pickInBrowser(page, MODEL);
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs in this tab. The first run downloads ${decimalSize(size)}, which this browser keeps. ${IN_MEMORY}; ` +
        'only a reload frees all of it.',
    );
    await loadPreset(page, 'Ticket routing');
    await expect(page.getByTestId('run-note')).toHaveText(firstDownload(size));

    // R2.2: the download with its bytes as one stage across the three files, then the start, then the answers. The
    // server holds the files, so the tab says nothing of a pull, though every request here waits 1 s (the server's
    // own signal, `x_browser_pulled`, says when it pulls: `browser_pull`). The pipe is narrowed for the first
    // download, so its progress can be photographed, and opened again once it has been.
    const cdp = await context.newCDPSession(page);
    await cdp.send('Network.enable');
    await throttle(cdp, 48, 1000);
    await watch(page);
    await runKey(page).click();
    await expect(page.getByTestId('run-note')).toContainText(`Downloading ${MODEL} into this tab`);
    // Run's description is the banner's words alone, the bar is hidden from assistive technology, and the status
    // region speaks.
    await expect(loading(page)).toHaveAccessibleDescription(downloading(size));
    await expect(page.locator('#run-note progress')).toHaveAttribute('aria-hidden', 'true');
    await expect(page.getByRole('status').filter({ hasText: `Downloading ${MODEL} into this tab` })).toHaveCount(1);
    await screenshot(page, testInfo, 'browser_run', 'download');
    await throttle(cdp, null);
    await expect.poll(async () => (await seen(page)).at(-1), { timeout: 600_000 }).toBe('Run | ');
    const first = await seen(page);
    const log = first.join('\n');
    const downloads = first
      .map((entry, at) => ({ entry, at }))
      .filter(({ entry }) => entry.startsWith(`Loading | Downloading ${MODEL} into this tab: `));
    const starting = first
      .map((entry, at) => ({ entry, at }))
      .filter(({ entry }) => entry === `Loading | Starting ${MODEL} in this tab`);
    const running = first.indexOf(`Running | Running ${MODEL} in this tab on WebGPU`);
    expect(first.filter((entry) => / \| Pulling /.test(entry)), log).toEqual([]);
    expect(downloads.length, log).toBeGreaterThan(2);
    // One Downloading stage across the three files, Starting once, after the files and before the session runs.
    expect(first.slice(downloads[0].at, downloads[downloads.length - 1].at + 1), log).toEqual(
      downloads.map(({ entry }) => entry),
    );
    expect(starting.map(({ at }) => at), log).toEqual([downloads[downloads.length - 1].at + 1]);
    expect(running, log).toBe(starting[0].at + 1);
    const bytes = downloads.map(({ entry }) => Number(entry.split(' | ')[2].split('/')[0]));
    expect(bytes).toEqual([...bytes].sort((a, b) => a - b));
    expect(bytes[bytes.length - 1]).toBe(size);
    expect(downloads[downloads.length - 1].entry).toContain(`${decimalSize(size)} of ${decimalSize(size)} | ${size}/${size}`);
    // The status region heard each stage once, the download in tenths, then the answer.
    const heard = (await said(page)).filter((text) => text !== '');
    const tenths = heard.filter((text) => text.startsWith(`Downloading ${MODEL} into this tab: `));
    expect(tenths, heard.join('\n')).toEqual(
      tenths.map((_, i) => `Downloading ${MODEL} into this tab: ${i * 10}% of ${decimalSize(size)}`),
    );
    expect(tenths.length, heard.join('\n')).toBe(11);
    expect(heard, heard.join('\n')).toEqual([
      ...tenths,
      `Starting ${MODEL} in this tab`,
      `Running ${MODEL} in this tab on WebGPU`,
      expect.stringMatching(new RegExp(`^Answered by decider-0\\.8b-v1 in this tab on WebGPU: 2 questions, \\d+ ms$`)),
    ]);

    // The answers are the tab's, read out as the server reads them; no request went to /v1/systemone.
    await expect(page.getByTestId('answered-by')).toContainText(' in this tab on WebGPU');
    await expect(page.getByTestId('fault')).toHaveCount(0);
    const webgpu = await tabResponse(page);
    expect(webgpu.model).toBe(server.model);
    // R2.3: the same top answers as the server's.
    expect(topAnswers(webgpu)).toEqual(topAnswers(server));
    await expectFiguresMatch(page, webgpu as unknown as Json);
    await expect(page.getByTestId('raw-status')).toHaveText('HTTP 200');
    await expect(page.getByRole('heading', { level: 3, name: 'Sent · in this tab' })).toBeVisible();
    // Kept now: the banner has nothing more to say.
    await expect(page.getByTestId('run-note')).toBeHidden();
    expect(requests.filter(isSystemOne)).toEqual([]);
    // R2.6: the runtime and the engine module came from this origin, and nothing from another.
    const paths = requests.map((url) => new URL(url).pathname);
    expect(paths).toContain('/ort/1.30.0/ort.webgpu.bundle.min.mjs');
    expect(paths).toContain('/ort/1.30.0/ort-wasm-simd-threaded.asyncify.wasm');
    expect(paths).toContain('/engine/ardana-engine.js');
    expect(paths).toContain('/engine/ardana-engine_bg.wasm');
    for (const url of requests) {
      expect(new URL(url).origin, url).toBe(origin);
    }
    await screenshot(page, testInfo, 'browser_run', 'answered');

    // A server row releases the model this tab loaded (its session and its reader), here picked the moment a row
    // starts decoding: the run stops, and the session is freed once that row is over (onnxruntime-web frees a session
    // at once, and the row would then read freed memory). Back in the tab, the next run loads the model again from the
    // files this browser kept: only the profile is fetched, and the answers are the same.
    await page.evaluate(WATCH_SESSIONS);
    await pickOnceRunning(page, MODEL);
    await runKey(page).click();
    await expect(picker(page)).toHaveValue(MODEL);
    // Stopped while a row decodes, the run had no download for the next one to resume.
    await expect(page.getByTestId('run-status')).toHaveText('Stopped, not answered');
    const ending = (events: string[]) => events.filter((event) => /^(release|run failed)/.test(event));
    await expect.poll(async () => ending(await sessionEvents(page)).length).toBeGreaterThan(0);
    const released = await sessionEvents(page);
    expect(ending(released), released.join('\n')).toEqual(['release']);
    // Stopped while it ran, the run had a row in flight, which ended before the release.
    expect(released.at(released.indexOf('release') - 1), released.join('\n')).toBe('ran');
    await pickInBrowser(page, MODEL);
    const before = requests.length;
    await runInTab(page);
    await expect(page.getByTestId('answered-by')).toContainText(' in this tab on WebGPU');
    expect(
      requests
        .slice(before)
        .map((url) => new URL(url).pathname)
        .filter((path) => path.startsWith('/v1/browser/')),
    ).toEqual([`/v1/browser/${MODEL}/profile`]);
    expect(await seen(page), (await seen(page)).join('\n')).toContain(`Loading | Starting ${MODEL} in this tab`);
    expect(topAnswers(await tabResponse(page))).toEqual(topAnswers(server));
    const events = await sessionEvents(page);
    expect(ending(events), events.join('\n')).toEqual(['release']);

    // R2.4: after a reload the browser has the files; only the profile is fetched, and the stages say so: no pull, no
    // download, no size line before the tap; and no pull though every request now waits 600 ms (round 5 saw this run
    // say "Pulling" for 200 ms on such a network, when a grace of 400 ms stood for the server's signal).
    const fetched: Promise<[string, number]>[] = [];
    const browserFiles = (response: { url(): string; request(): { sizes(): Promise<{ responseBodySize: number }> } }) => {
      const path = new URL(response.url()).pathname;
      if (path.startsWith('/v1/browser/')) {
        fetched.push(response.request().sizes().then((sizes) => [path, sizes.responseBodySize]));
      }
    };
    page.on('response', browserFiles);
    await page.reload();
    await pickInBrowser(page, MODEL);
    await loadPreset(page, 'Ticket routing');
    await expect(page.getByTestId('model-note')).toBeVisible();
    await expect(page.getByTestId('run-note')).toBeHidden();
    await throttle(cdp, null, 600);
    await runInTab(page);
    await throttle(cdp, null);
    await expect(page.getByTestId('answered-by')).toContainText(' in this tab on WebGPU');
    const kept = await seen(page);
    expect(kept.filter((entry) => / \| (Pulling|Downloading) /.test(entry)), kept.join('\n')).toEqual([]);
    expect(kept, kept.join('\n')).toContain(`Loading | Starting ${MODEL} in this tab`);
    expect((await said(page)).filter((text) => text.startsWith('Pulling'))).toEqual([]);
    const second = await Promise.all(fetched);
    page.off('response', browserFiles);
    expect(second.map(([path]) => path)).toEqual([`/v1/browser/${MODEL}/profile`]);
    const downloaded = second.reduce((sum, [, bytes]) => sum + bytes, 0);
    expect(downloaded).toBeLessThan(1_000_000);
    expect(topAnswers(await tabResponse(page))).toEqual(topAnswers(server));
    console.log(`browser_run ${testInfo.project.name}: first run ${size} bytes of model files, after a reload ${downloaded}`);

    // R2.3: without WebGPU the tab runs on the WASM build, to the same top answers.
    const wasm = await context.newPage();
    await wasm.addInitScript(() => {
      delete (Navigator.prototype as unknown as { gpu?: unknown }).gpu;
    });
    const wasmRequests: string[] = [];
    wasm.on('request', (r) => wasmRequests.push(r.url()));
    await wasm.goto('/');
    expect(await wasm.evaluate(() => 'gpu' in navigator)).toBe(false);
    await pickInBrowser(wasm, MODEL);
    await loadPreset(wasm, 'Ticket routing');
    await runInTab(wasm);
    await expect(wasm.getByTestId('answered-by')).toContainText(' in this tab on WASM');
    const onWasm = await tabResponse(wasm);
    expect(topAnswers(onWasm)).toEqual(topAnswers(server));
    await expectFiguresMatch(wasm, onWasm as unknown as Json);
    const wasmPaths = wasmRequests.map((url) => new URL(url).pathname);
    expect(wasmPaths).toContain('/ort/1.30.0/ort.wasm.bundle.min.mjs');
    expect(wasmPaths).toContain('/ort/1.30.0/ort-wasm-simd-threaded.wasm');
    expect(wasmPaths.filter((path) => path === '/v1/systemone')).toEqual([]);
    for (const url of wasmRequests) {
      expect(new URL(url).origin, url).toBe(origin);
    }
    await screenshot(wasm, testInfo, 'browser_run', 'wasm');
    await wasm.close();
    console.log(
      `browser_run ${testInfo.project.name}: top answers server ${JSON.stringify(topAnswers(server))}, ` +
        `WebGPU ${JSON.stringify(topAnswers(webgpu))}, WASM ${JSON.stringify(topAnswers(onWasm))}`,
    );

    // R2.5: a question that fails validation shows in the tab the 422 body the server returns for that request.
    const invalid: Request = structuredClone(request);
    (invalid.questions.department as Record<string, Json>).criteria = ['billing'];
    const refused = await context.request.post('/v1/systemone', { data: invalid });
    expect(refused.status()).toBe(422);
    const refusal = await refused.text();
    const sent = requests.length;
    await page.goto(`/${shareHash(invalid)}`);
    await pickInBrowser(page, MODEL);
    await runInTab(page);
    await expect(page.getByTestId('raw-status')).toHaveText('HTTP 422');
    await expect(page.getByTestId('raw-response')).toHaveJSProperty('textContent', refusal);
    const detail = (JSON.parse(refusal) as { detail: { msg: string }[] }).detail;
    await expect(page.getByTestId('fault-issue')).toHaveCount(detail.length);
    await expect(page.getByTestId('fault-issue').first()).toContainText(detail[0].msg);
    expect(requests.slice(sent).filter(isSystemOne)).toEqual([]);
    console.log(`browser_run ${testInfo.project.name}: the tab's 422 body equals the server's: ${refusal}`);
    await screenshot(page, testInfo, 'browser_run');
  } finally {
    await context.close();
  }
});

// A page that is no secure context (a host name other than localhost, over HTTP: `--host 0.0.0.0` opened from another
// device) has no Cache Storage. The run downloads without keeping the files, on WASM (no WebGPU there either), and
// nothing on the page says the files are kept.
test('insecure_origin', async ({}, testInfo) => {
  test.setTimeout(900_000);
  expect(base, 'xtask passes the server of this case').toBeTruthy();
  const origin = `http://insecure.test:${new URL(base).port}`;
  const browser = await chromium.launch({
    channel: 'chrome',
    args: ['--host-resolver-rules=MAP insecure.test 127.0.0.1'],
  });
  try {
    const context = await browser.newContext({ viewport: testInfo.project.use.viewport, baseURL: origin });
    const page = await context.newPage();
    const requests: string[] = [];
    page.on('request', (r) => requests.push(r.url()));
    // The test's own requests resolve no host names of the browser's: they go to the server's address.
    const size = await browserSize(page, base);
    await page.goto('/');
    expect(await page.evaluate(() => [window.isSecureContext, 'caches' in window])).toEqual([false, false]);
    await pickInBrowser(page, MODEL);
    await loadPreset(page, 'Ticket routing');
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs in this tab. The first run on each visit downloads ${decimalSize(size)}: a page without HTTPS keeps no ` +
        `files. ${IN_MEMORY}; only a reload frees all of it.`,
    );
    await expect(page.getByTestId('run-note')).toHaveText(
      `Run downloads ${MODEL} into this tab on each visit: ${decimalSize(size)}, as a page without HTTPS keeps no ` +
        `files. ${IN_MEMORY}.`,
    );
    await screenshot(page, testInfo, 'insecure_origin', 'opened');

    // Stopped mid-download, the run keeps nothing here, and says the next run starts the download over.
    const ranged: string[] = [];
    page.on('request', (r) => {
      if (r.headers().range) {
        ranged.push(`${new URL(r.url()).pathname} ${r.headers().range}`);
      }
    });
    const cdp = await context.newCDPSession(page);
    await cdp.send('Network.enable');
    await throttle(cdp, 32);
    await runKey(page).click();
    await expect(page.locator('#run-note progress')).toHaveAttribute('value', /^[1-9]\d{7,}$/, { timeout: 600_000 });
    await page.getByTestId('run-note').getByRole('button', { name: 'Stop' }).click();
    await expect(page.getByTestId('run-status')).toHaveText(
      'Stopped, not answered. The next run starts the download over: a page without HTTPS keeps no files.',
    );
    await throttle(cdp, null);

    await runKey(page).click();
    await expect(page.getByTestId('answered-by')).toContainText(' in this tab on WASM', { timeout: 600_000 });
    expect(ranged).toEqual([]);
    await expect(page.getByTestId('run-label')).toHaveText('Run');
    await expect(page.getByTestId('fault')).toHaveCount(0);
    const answered = await tabResponse(page);
    expect(answered.model).toBe('decider-0.8b-v1');
    await expectFiguresMatch(page, answered as unknown as Json);
    const text = await visibleText(page);
    for (const claim of ['which this browser keeps', 'kept by this browser']) {
      expect(text, claim).not.toContain(claim);
    }
    expect(requests.filter(isSystemOne)).toEqual([]);
    for (const url of requests) {
      expect(new URL(url).origin, url).toBe(origin);
    }
    await screenshot(page, testInfo, 'insecure_origin');
  } finally {
    await browser.close();
  }
});

// A first run in the tab is a download nobody has agreed to until Run is pressed: an autorun share link waits for the
// tap, the banner says what Run downloads at every width, Stop ends a download where it is, and so does picking another
// row; nothing a stopped run would have answered lands.
test('browser_stop', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  expect(empty, 'xtask passes the empty server').toBeTruthy();
  const size = await browserSize(page, empty);
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));
  const aborted: string[] = [];
  page.on('requestfailed', (r) => {
    if (r.failure()?.errorText === 'net::ERR_ABORTED') {
      aborted.push(new URL(r.url()).pathname);
    }
  });
  const files = () => requests.filter((url) => /^\/v1\/browser\/[^/]+\/model\.onnx/.test(new URL(url).pathname));

  // An autorun link onto decider:0.8b's "In browser" row, with nothing kept: the banner says what Run downloads, Run
  // waits for the tap, and nothing is fetched.
  const ticket = fixture('ticket.json');
  await page.goto(`${empty}/?autorun=1${shareHash({ ...ticket, model: MODEL })}`);
  await expect(picker(page)).toHaveValue(`${MODEL} in-browser`);
  await expect(page.getByTestId('run-note')).toHaveText(firstDownload(size));
  await expect(page.getByTestId('run-note')).toBeInViewport();
  await expect(runKey(page)).toHaveAccessibleDescription(firstDownload(size));
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(runKey(page)).toHaveAttribute('aria-busy', 'false');
  expect(files()).toEqual([]);
  expect(requests.filter((url) => new URL(url).pathname.startsWith('/v1/browser/'))).toEqual([]);
  await screenshot(page, testInfo, 'browser_stop', 'waiting');

  // The tap starts the download; Stop ends it where it is: the request is cancelled, no other starts, Run says Run
  // again with the focus on it, and no answer lands.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await throttle(cdp, 16);
  const tapped = requests.length;
  await runKey(page).click();
  await expect(page.getByTestId('run-note')).toContainText(`Downloading ${MODEL} into this tab`);
  await expect(loading(page)).toHaveAccessibleDescription(downloading(size));
  await expect.poll(() => files().some((url) => url.endsWith('/model.onnx.data'))).toBe(true);
  const stop = page.getByTestId('run-note').getByRole('button', { name: 'Stop' });
  await expect(stop).toBeInViewport();
  await screenshot(page, testInfo, 'browser_stop', 'download');
  const before = requests.length;
  await stop.click();
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(runKey(page)).toBeFocused();
  await expect.poll(() => aborted.filter((path) => path.endsWith('/model.onnx.data'))).toHaveLength(1);
  await expect(page.getByTestId('run-status')).toHaveText(STOPPED);
  await expect(page.getByTestId('run-note')).toHaveText(firstDownload(size));
  await expect(page.getByTestId('answered-by')).toHaveCount(0);
  await expect(page.getByTestId('fault')).toHaveCount(0);
  await expect(page.getByTestId('raw-response')).toHaveCount(0);
  expect(requests.slice(before).filter((url) => new URL(url).pathname.startsWith('/v1/browser/'))).toEqual([]);
  // The autorun asked for nothing before the tap.
  expect(requests.slice(0, tapped).filter((url) => new URL(url).pathname.startsWith('/v1/browser/'))).toEqual([]);
  await screenshot(page, testInfo, 'browser_stop', 'stopped');

  // Picking another row stops a download as Stop does: here decider:2b's server row, which this server has not
  // pulled (Run is then held for it).
  await runKey(page).click();
  await expect(page.getByTestId('run-note')).toContainText(`Downloading ${MODEL} into this tab`);
  await pickModel(page, 'decider:2b');
  await expect.poll(() => aborted.filter((path) => path.endsWith('/model.onnx.data'))).toHaveLength(2);
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(page.getByTestId('run-status')).toHaveText(STOPPED);
  await expect(page.getByTestId('run-note')).toContainText('This server has not pulled decider:2b');
  await expect(page.getByTestId('answered-by')).toHaveCount(0);
  await throttle(cdp, null);
  expect(requests.filter(isSystemOne)).toEqual([]);
  const listed = (await (await page.request.get(`${empty}/v1/models`)).json()) as { models: ModelInfo[] };
  expect(listed.models.every((m) => m.x_pulled === false)).toBe(true);
  await screenshot(page, testInfo, 'browser_stop');
});

// A run in the tab that loses its connection says what failed and what to do next, and, the connection back, Run
// answers without a reload.
test('browser_recover', async ({ page }, testInfo) => {
  test.setTimeout(900_000);
  expect(empty, 'xtask passes the empty server').toBeTruthy();
  await page.goto(`${empty}/`);
  await pickInBrowser(page, MODEL);
  await loadPreset(page, 'Ticket routing');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await throttle(cdp, 16);
  await runKey(page).click();
  await expect(page.getByTestId('run-note')).toContainText(`Downloading ${MODEL} into this tab`);
  await cdp.send('Network.emulateNetworkConditions', {
    offline: true,
    latency: 0,
    downloadThroughput: -1,
    uploadThroughput: -1,
  });
  await expect(page.getByTestId('fault')).toBeVisible({ timeout: 600_000 });
  await expect(page.getByTestId('fault')).toContainText('Not answered in this tab.');
  await expect(page.getByTestId('fault-advice')).toHaveText(/ Run again once the connection is back( to resume it)?\.$/);
  for (const raw of ['Aborted(', '-sASSERTIONS', 'Failed to fetch']) {
    await expect(page.getByTestId('fault')).not.toContainText(raw);
  }
  // The reason, as the browser gave it, is the raw exchange's.
  await expect(page.getByTestId('raw-response')).not.toBeEmpty();
  await expect(page.getByTestId('run-status')).toHaveText(
    /^Not answered in this tab\. .+ Run again once the connection is back( to resume it)?\.$/,
  );
  await screenshot(page, testInfo, 'browser_recover', 'failed');

  // Back online, but onnxruntime-web's files out of reach: the download finishes, the runtime does not load, and the
  // fault says so. A build that did not load is not kept: once its files are back, Run imports it again and answers,
  // on WebGPU, whose build is asked for again under a new query (round 5 saw every later run stay on WASM).
  const requests: string[] = [];
  page.on('request', (r) => requests.push(new URL(r.url()).pathname + new URL(r.url()).search));
  const bundle = '/ort/1.30.0/ort.wasm.bundle.min.mjs';
  await throttle(cdp, null);
  await cdp.send('Network.setBlockedURLs', { urls: ['*/ort/1.30.0/*'] });
  await runKey(page).click();
  await expect(page.getByTestId('fault-advice')).toHaveText(
    'onnxruntime-web did not load into this tab. Run again once the connection is back.',
    { timeout: 600_000 },
  );
  expect(requests.filter((path) => path.split('?')[0] === bundle)).toHaveLength(1);
  await cdp.send('Network.setBlockedURLs', { urls: [] });
  await runKey(page).click();
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on WebGPU', { timeout: 600_000 });
  await expect(page.getByTestId('fault')).toHaveCount(0);
  expect(requests.filter((path) => /^\/ort\/1\.30\.0\/ort\.webgpu\.bundle\.min\.mjs\?retry=\d+$/.test(path)).length).toBeGreaterThan(0);
  const answered = await tabResponse(page);
  expect(answered.model).toBe('decider-0.8b-v1');
  await expectFiguresMatch(page, answered as unknown as Json);

  // On a page of its own, the WebGPU build's WASM module cannot be fetched, once: that run answers on WASM, and the next
  // imports the build again under a new query and answers on WebGPU (onnxruntime-web keeps a backend's failed start for
  // the life of its module: without a new import, every later run stayed on WASM until a reload).
  const gpu = await page.context().newPage();
  const gpuCdp = await page.context().newCDPSession(gpu);
  await gpuCdp.send('Network.enable');
  await gpuCdp.send('Network.setCacheDisabled', { cacheDisabled: true });
  await gpuCdp.send('Network.setBlockedURLs', { urls: ['*/ort-wasm-simd-threaded.asyncify.wasm*'] });
  const gpuRequests: string[] = [];
  gpu.on('request', (r) => gpuRequests.push(new URL(r.url()).pathname + new URL(r.url()).search));
  await gpu.goto(`${empty}/`);
  await expect(picker(gpu)).toHaveValue(`${MODEL} in-browser`);
  await loadPreset(gpu, 'Ticket routing');
  await runKey(gpu).click();
  await expect(gpu.getByTestId('run-status')).toHaveText(/^Answered by decider-0\.8b-v1 in this tab on WASM: /, {
    timeout: 600_000,
  });
  await gpuCdp.send('Network.setBlockedURLs', { urls: [] });
  await runKey(gpu).click();
  await expect(gpu.getByTestId('run-status')).toHaveText(/^Answered by decider-0\.8b-v1 in this tab on WebGPU: /, {
    timeout: 600_000,
  });
  expect(gpuRequests).toContain('/ort/1.30.0/ort.webgpu.bundle.min.mjs?retry=1');
  await screenshot(gpu, testInfo, 'browser_recover', 'webgpu-again');
  await gpu.close();

  // Ardana's engine out of reach on a page of its own: the run says so and downloads no file; once the engine is back,
  // Run imports it again under a new query and answers, without a reload.
  const fresh = await page.context().newPage();
  const freshCdp = await page.context().newCDPSession(fresh);
  await freshCdp.send('Network.enable');
  await freshCdp.send('Network.setBlockedURLs', { urls: ['*/engine/ardana-engine*'] });
  const imported: string[] = [];
  fresh.on('request', (r) => imported.push(r.url()));
  await fresh.goto(`${empty}/`);
  await expect(picker(fresh)).toHaveValue(`${MODEL} in-browser`);
  await loadPreset(fresh, 'Ticket routing');
  await runKey(fresh).click();
  await expect(fresh.getByTestId('fault-advice')).toHaveText(
    "Ardana's engine did not load into this tab. Run again once the connection is back.",
  );
  const files = () => imported.filter((url) => /\/v1\/browser\/[^/]+\/(tokenizer\.json|model\.onnx)/.test(url));
  expect(files()).toEqual([]);
  await freshCdp.send('Network.setBlockedURLs', { urls: [] });
  await runKey(fresh).click();
  await expect(fresh.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  await expect(fresh.getByTestId('fault')).toHaveCount(0);
  expect(imported.filter((url) => /\/engine\/ardana-engine\.js\?retry=\d+$/.test(url)).length).toBeGreaterThan(0);
  await fresh.close();
  await screenshot(page, testInfo, 'browser_recover');
});

/**
 * A proxy in front of the server at `target`, on a port of its own: it forwards every request as sent and every response
 * as the server sends it, and `sever()` cuts the responses of the weights in flight, as a dropped connection does
 * (Chrome's own offline emulation leaves a response that is under way alone).
 */
async function severable(target: string) {
  const upstream = new URL(target);
  const weights = new Set<{ reply: http.IncomingMessage; res: http.ServerResponse }>();
  const proxy = http.createServer((req, res) => {
    const forward = http.request(
      { host: upstream.hostname, port: upstream.port, path: req.url, method: req.method, headers: req.headers },
      (reply) => {
        res.writeHead(reply.statusCode!, reply.headers);
        if (req.url?.endsWith('/model.onnx.data')) {
          const stream = { reply, res };
          weights.add(stream);
          res.on('close', () => weights.delete(stream));
        }
        reply.pipe(res);
      },
    );
    forward.on('error', () => res.destroy());
    req.pipe(forward);
  });
  await new Promise<void>((listening) => proxy.listen(0, '127.0.0.1', listening));
  return {
    url: `http://127.0.0.1:${(proxy.address() as AddressInfo).port}`,
    sever: () => {
      for (const { reply, res } of weights) {
        reply.destroy();
        res.destroy();
      }
    },
    close: () =>
      new Promise<void>((closed) => {
        proxy.closeAllConnections();
        proxy.close(() => closed());
      }),
  };
}

/** Waits until the download bar of a run in the tab counts more than `bytes`. */
async function barPast(page: Page, bytes: number) {
  await expect
    .poll(() => page.locator('#run-note progress').evaluate((bar: HTMLProgressElement) => bar.value).catch(() => 0), {
      timeout: 600_000,
    })
    .toBeGreaterThan(bytes);
}

// A download of the weights that stops (the connection drops, Stop, a reload) resumes where this browser's Cache Storage
// kept it: the next run asks for the rest (`Range: bytes=<had>-`, `If-Range` on the version), the server sends it
// (206), and over three interruptions the weights cross the wire about once. The page runs behind a proxy in front of
// the browser server, in a persistent profile (an incognito context's Cache Storage cannot hold the weights).
test('browser_resume', async ({}, testInfo) => {
  test.setTimeout(900_000);
  expect(base, 'xtask passes the server of this case').toBeTruthy();
  const proxy = await severable(base);
  const profile = testInfo.outputPath('profile');
  fs.rmSync(profile, { recursive: true, force: true });
  const context = await chromium.launchPersistentContext(profile, {
    channel: 'chrome',
    viewport: testInfo.project.use.viewport,
    baseURL: proxy.url,
  });
  try {
    // The server's own answer to the same request, from decider:0.8b's GGUF, and the length of the weights.
    const ticket = fixture('ticket.json');
    const served = await context.request.post(`${base}/v1/systemone`, { data: { ...ticket, model: MODEL } });
    expect(served.status()).toBe(200);
    const server = (await served.json()) as Response;
    const size = await browserSize(context.pages()[0] ?? (await context.newPage()), base);
    const weights = Number(
      (await context.request.head(`${base}/v1/browser/${MODEL}/model.onnx.data`)).headers()['content-length'],
    );
    expect(weights).toBeGreaterThan(400_000_000);

    const page = context.pages()[0];
    const cdp = await context.newCDPSession(page);
    await cdp.send('Network.enable');
    // Every request for the weights, and every byte of them this page receives, over every run.
    const responses: PlaywrightResponse[] = [];
    page.on('response', (response) => {
      if (new URL(response.url()).pathname.endsWith('/model.onnx.data')) {
        responses.push(response);
      }
    });
    const weightRequests = new Set<string>();
    let received = 0;
    cdp.on('Network.requestWillBeSent', (event) => {
      if (event.request.url.endsWith('/model.onnx.data')) {
        weightRequests.add(event.requestId);
      }
    });
    cdp.on('Network.dataReceived', (event) => {
      if (weightRequests.has(event.requestId)) {
        received += event.dataLength;
      }
    });
    const open = async () => {
      await pickInBrowser(page, MODEL);
      await loadPreset(page, 'Ticket routing');
    };
    await page.goto('/');
    await open();

    // The connection drops about 150 MB in: the fault says the next run resumes the download.
    await throttle(cdp, 48);
    await runKey(page).click();
    await barPast(page, 150_000_000);
    proxy.sever();
    await expect(page.getByTestId('fault-advice')).toHaveText(
      new RegExp(
        `^The download of decider:0\\.8b stopped at [\\d.]+ MB of ${decimalSize(size).replace('.', '\\.')}\\. ` +
          'Run again once the connection is back to resume it\\.$',
      ),
      { timeout: 60_000 },
    );
    await screenshot(page, testInfo, 'browser_resume', 'severed');

    // Run resumes after what this browser kept; Stop ends it about 280 MB in, and says the next run resumes again.
    await runKey(page).click();
    await barPast(page, 280_000_000);
    await page.getByTestId('run-note').getByRole('button', { name: 'Stop' }).click();
    await expect(page.getByTestId('run-status')).toHaveText(STOPPED);
    await screenshot(page, testInfo, 'browser_resume', 'stopped');

    // Run resumes again; a reload about 380 MB in ends it.
    await runKey(page).click();
    await barPast(page, 380_000_000);
    await page.reload();
    await open();

    // After the reload, Run resumes once more, takes the rest and answers as the server does.
    await throttle(cdp, null);
    await runKey(page).click();
    await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
    await expect(page.getByTestId('fault')).toHaveCount(0);
    const answered = await tabResponse(page);
    expect(topAnswers(answered)).toEqual(topAnswers(server));
    await expectFiguresMatch(page, answered as unknown as Json);

    // Four requests for the weights: the whole file, then three times the rest after what the tab had, each from
    // further on, each answered with that rest (206).
    const asked = responses.map((response) => ({
      range: response.request().headers().range,
      status: response.status(),
      contentRange: response.headers()['content-range'],
    }));
    const log = JSON.stringify(asked);
    expect(asked.length, log).toBe(4);
    expect([asked[0].range, asked[0].status], log).toEqual([undefined, 200]);
    let had = 0;
    for (const resumed of asked.slice(1)) {
      const from = Number(/^bytes=(\d+)-$/.exec(resumed.range ?? '')?.[1]);
      expect(from, log).toBeGreaterThan(had);
      expect(resumed.status, log).toBe(206);
      expect(resumed.contentRange, log).toBe(`bytes ${from}-${weights - 1}/${weights}`);
      had = from;
    }
    // The last request took the rest of the file, and nothing more; over every run the weights crossed the wire about
    // once (each interruption costs at most the part it was receiving).
    expect((await responses[3].request().sizes()).responseBodySize, log).toBe(weights - had);
    expect(received, log).toBeGreaterThanOrEqual(weights);
    expect(received, log).toBeLessThan(weights + 3 * 16 * 1024 * 1024);

    // This browser keeps each file once, whole, and none of the parts it came in.
    const kept = await page.evaluate(async () => {
      const names = (await caches.keys()).filter((name) => name.startsWith('ardana-browser decider:0.8b '));
      const cache = await caches.open(names[0]);
      const keys = (await cache.keys()).map((request) => new URL(request.url).pathname + new URL(request.url).search);
      const blob = await (await cache.match('/v1/browser/decider:0.8b/model.onnx.data'))!.blob();
      return { names, keys: keys.sort(), weights: blob.size };
    });
    expect(kept.names).toHaveLength(1);
    expect(kept.keys).toEqual([
      '/v1/browser/decider:0.8b/model.onnx',
      '/v1/browser/decider:0.8b/model.onnx.data',
      '/v1/browser/decider:0.8b/tokenizer.json',
    ]);
    expect(kept.weights).toBe(weights);
    console.log(
      `browser_resume ${testInfo.project.name}: resumed at ${asked
        .slice(1)
        .map((resumed) => resumed.range)
        .join(', ')}; ${received} bytes of ${weights} received over four runs`,
    );
    await screenshot(page, testInfo, 'browser_resume');
  } finally {
    await context.close();
    await proxy.close();
  }
});

// The server says when it pulls: a server whose Hugging Face cache holds no browser variant lists none as pulled
// (`x_browser_pulled`), and the first run in the tab says the server pulls the files, with Stop, for as long as the
// server takes (every request here waits 1.5 s); offline over an empty cache that pull fails, and the run says the
// server could not provide the files.
test('browser_pull', async ({ page }, testInfo) => {
  test.setTimeout(300_000);
  const uncached = process.env.ARDANA_UNCACHED_URL!;
  expect(uncached, 'xtask passes the server over an empty cache').toBeTruthy();
  const listed = (await (await page.request.get(`${uncached}/v1/models`)).json()) as { models: ModelInfo[] };
  const browserModels = listed.models.filter((m) => m.x_browser !== undefined);
  expect(browserModels.map((m) => m.name).sort()).toEqual(['decider:0.8b', 'decider:2b', 'qwen3.5:0.8b']);
  expect(browserModels.filter((m) => m.x_browser_pulled !== undefined)).toEqual([]);
  const size = browserModels.find((m) => m.name === MODEL)!.x_browser!;

  await page.goto(`${uncached}/`);
  await expect(picker(page)).toHaveValue(`${MODEL} in-browser`);
  await loadPreset(page, 'Ticket routing');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await throttle(cdp, null, 1500);
  await watch(page);
  await runKey(page).click();
  const pulling = `Pulling the browser files of ${MODEL} (${decimalSize(size)}) on the server`;
  await expect(page.locator('#run-note-text')).toHaveText(pulling);
  await expect(page.getByTestId('run-label')).toHaveText('Pulling');
  await expect(page.getByTestId('run-note').getByRole('button', { name: 'Stop' })).toBeVisible();
  await screenshot(page, testInfo, 'browser_pull', 'pulling');
  const advice = `The server could not provide the browser files of ${MODEL}. Try again later.`;
  await expect(page.getByTestId('fault-advice')).toHaveText(advice, { timeout: 60_000 });
  await throttle(cdp, null);
  const heard = (await said(page)).filter((text) => text !== '');
  expect(heard, heard.join('\n')).toEqual([pulling, `Not answered in this tab. ${advice}`]);
  await screenshot(page, testInfo, 'browser_pull');
});

// WCAG 2.4.11 while a run in the tab downloads: the banner stays under the top bar, as tall as its words, bar and Stop
// make it at that width and text size, and whatever takes the focus, by Tab or Shift+Tab, lands below both. The page
// holds a server run's answers (their fact terms take the focus) while decider:0.8b downloads, narrowed to 2 MB/s.
test('banner_focus', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const ticket = fixture('ticket.json');
  const paths: string[] = [];
  page.on('request', (r) => paths.push(new URL(r.url()).pathname));
  await page.goto(`/?autorun=1${shareHash({ ...ticket, model: 'decider:2b' })}`);
  await expect(page.getByTestId('answered-by')).toContainText('Answered by', { timeout: 120_000 });
  // A page that runs a server model has loaded neither the engine module nor onnxruntime-web (only `engine.js`, its
  // own glue); an "In browser" pick imports both.
  const deferred = (path: string) => path.startsWith('/engine/ardana-engine') || path.startsWith('/ort/');
  expect(paths.filter(deferred)).toEqual([]);
  // The last question's builder open too, so its text fields take the focus on the way back.
  await page.getByRole('button', { name: /^Edit question / }).last().click();
  await pickInBrowser(page, MODEL);
  await expect.poll(() => paths.filter((path) => path.startsWith('/engine/'))).toEqual(
    expect.arrayContaining(['/engine/ardana-engine.js', '/engine/ardana-engine_bg.wasm']),
  );
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await throttle(cdp, 2);
  await runKey(page).click();
  await expect(page.locator('#run-note progress')).toBeVisible();
  const own = testInfo.project.use.viewport!;
  // The project's viewport at the default text size and with 200% text; on the phone project also the narrowest and
  // the widest phones.
  const sizes: [number, number, boolean][] = [
    [own.width, own.height, false],
    [own.width, own.height, true],
  ];
  if (own.width < 720) {
    sizes.push([320, 640, false], [320, 640, true], [430, 932, false]);
  }
  for (const [width, height, large] of sizes) {
    await page.setViewportSize({ width, height });
    const text = large ? await page.addStyleTag({ content: 'html { font-size: 200% !important; }' }) : null;
    const key = `${width}x${height}${large ? ' with 200% text' : ''}`;
    // Backwards from Add question over the builder and the answers, then forwards from the state editor to Snippets.
    for (const [from, back] of [['#add-question', true], ['#state', false]] as const) {
      const stops = await walkFocus(page, from, back, 24);
      expect(stops.length, key).toBeGreaterThan(0);
      expect(
        stops.filter((stop) => !stop.clear),
        `${key}, ${back ? 'Shift+Tab' : 'Tab'}`,
      ).toEqual([]);
    }
    await text?.evaluate((tag) => tag.remove());
  }
  await page.setViewportSize(own);
  await expect(page.locator('#run-note progress')).toBeVisible();
  await screenshot(page, testInfo, 'banner_focus');
  await page.getByTestId('run-note').getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await throttle(cdp, null);
});
