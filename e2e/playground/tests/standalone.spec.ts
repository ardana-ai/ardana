// The standalone playground (`cargo xtask e2e standalone`, tagged @standalone): the build of
// `cargo xtask build-playground --public-url /playground/ --hub <stand-in>` as static files under `/playground/`
// (`ARDANA_BASE_URL`), with no server behind it, the library document served beside it at `/models.json` from the file
// `ARDANA_SITE_LIBRARY` (a copy of the snapshot, which these cases rewrite and take away), and a stand-in of Hugging
// Face on another origin serving `tmp/hf` offline (`ARDANA_HUB_URL`). The page bakes no library (Q13): it GETs
// `/models.json` on load and whenever its tab comes back, lists the document as a server that pulled nothing lists the
// library (one row per size under its canonical name `<family>:<size>`, in the document's order), runs the browser
// variants in the tab from the stand-in's repositories at the commits the document pins, and hands every other model
// to the ardana CLI.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { chromium, expect, test, type Page } from '@playwright/test';
import {
  IN_MEMORY,
  INSTALL,
  ardanaCommand,
  body,
  browserRow,
  decimalSize,
  expectFiguresMatch,
  expectSendsNothing,
  fixture,
  loadPreset,
  pickModel,
  picker as modelPicker,
  questionsBox,
  root,
  runKey,
  screenshot,
  shareHash,
  startsWith,
  stateBox,
  topAnswers,
  visibleText,
  type Json,
  type Request,
} from './helpers';

const MODEL = 'decider:0.8b';
const PAGE = '/playground/';
const LIBRARY = '/models.json';
const hub = process.env.ARDANA_HUB_URL!;
/** The file the site serves at `/models.json`. */
const served = process.env.ARDANA_SITE_LIBRARY!;

/** A library document (C1), the fields these cases read: families with their sizes inside. */
type Browser = { repo: string; commit: string; bytes: number };
type Size = {
  size: string;
  min_version?: string;
  gguf: { repo: string; commit: string; default: string; quants: { quant: string; file: string; bytes: number }[] };
  browser?: Browser;
  [key: string]: unknown;
};
type Family = { name: string; min_version?: string; sizes: Size[]; [key: string]: unknown };
type Document = { schema: number; default: string; browser_default?: string; models: Family[] };
/** A size as the page lists it: its canonical name, its default quant's bytes and its browser variant. */
type Row = { name: string; bytes: number; browser?: Browser };

/** The document the site serves now. */
const document = (): Document => JSON.parse(fs.readFileSync(served, 'utf8')) as Document;
/** The library snapshot (C5), which the served copy starts as. */
const snapshot = (): Document =>
  JSON.parse(fs.readFileSync(path.join(root, 'crates/ardana-registry/tests/data/models.json'), 'utf8')) as Document;
/** Replaces the served document. */
const serve = (library: Document) => fs.writeFileSync(served, JSON.stringify(library, null, 2));
/** The sizes of `library` in its order, as a server that pulled nothing lists them. */
const rows = (library: Document): Row[] =>
  library.models.flatMap((family) =>
    family.sizes.map((size) => ({
      name: `${family.name}:${size.size}`,
      bytes: size.gguf.quants.find((quant) => quant.quant === size.gguf.default)!.bytes,
      browser: size.browser,
    })),
  );
/** The size of `library` named `name`. */
const sized = (library: Document, name: string): Row => rows(library).find((r) => r.name === name)!;
/** The picker's row of a library size: its canonical name and the ardana CLI's download. */
const row = (r: Row) => `${r.name} · ${decimalSize(r.bytes)}`;
/** The picker's "Library" rows, every size of the document in its order. */
const libraryRows = (page: Page) =>
  modelPicker(page).locator('optgroup[label="Library · runs with the ardana CLI"] option');

type Response = { model: string; answers: Record<string, Record<string, Json>> };

/** The page's own answer: the exact body its raw panel shows. */
async function tabResponse(page: Page): Promise<Response> {
  return JSON.parse((await page.getByTestId('raw-response').textContent())!) as Response;
}

/** Every request `page` makes, as URLs, and the responses that failed or did not arrive. */
function record(page: Page): { requests: string[]; failed: string[] } {
  const requests: string[] = [];
  const failed: string[] = [];
  page.on('request', (r) => requests.push(r.url()));
  page.on('requestfailed', (r) => failed.push(`${r.url()}: ${r.failure()?.errorText}`));
  page.on('response', (r) => {
    if (r.status() >= 400) {
      failed.push(`${r.url()}: HTTP ${r.status()}`);
    }
  });
  return { requests, failed };
}

/** Whether `url` is the page's own: a file under `/playground/`, or the library document beside it. */
const own = (url: string) => new URL(url).pathname.startsWith(PAGE) || new URL(url).pathname === LIBRARY;
/** How many times the page asked for the library document. */
const libraryGets = (requests: string[]) => requests.filter((url) => new URL(url).pathname === LIBRARY).length;

/**
 * The tab comes back: headless Chrome keeps every page visible and focused (a tab switch sends nothing), so the case
 * sends the page one of the events a tab switch sends, `focus` at the window or `visibilitychange` at the document.
 */
const comeBack = (page: Page, event: 'focus' | 'visibilitychange') =>
  page.evaluate((event) => {
    if (event === 'focus') {
      window.dispatchEvent(new FocusEvent('focus'));
    } else {
      window.document.dispatchEvent(new Event('visibilitychange', { bubbles: true }));
    }
  }, event);

/**
 * The server row of `model` (or a name the library lacks): Run is held and says the model runs with the ardana CLI on
 * the visitor's machine, offering the browser default in this tab instead; the snippets show the install line and
 * `ardana run` with the exact request, and Run sends nothing.
 */
async function expectInstall(page: Page, model: string, request: Request, requests: string[]) {
  await expect(modelPicker(page)).toHaveValue(model);
  const reason = `${model} runs with the ardana CLI on your machine`;
  await expect(page.getByTestId('run-note')).toContainText(reason);
  await expect(runKey(page)).toHaveAccessibleDescription(startsWith(reason));
  await expect(
    page.getByTestId('run-note').getByRole('button', { name: `Run ${MODEL} in this tab instead` }),
  ).toBeVisible();
  await expect(page.getByTestId('cli-lede')).toHaveText(`${reason}.`);
  await expect(page.getByTestId('install')).toHaveText(INSTALL);
  await expect(page.getByTestId('pull')).toHaveCount(0);
  const command = await ardanaCommand(page);
  expect(command.line).toBe(`ardana run ${model} --request - <<'JSON'`);
  expect(command.body).toBe(body(model, request));
  const sent = requests.length;
  await expectSendsNothing(page, requests);
  expect(requests.slice(sent)).toEqual([]);
}

test.describe('standalone', { tag: '@standalone' }, () => {
  // Each case starts from the snapshot's document, whatever the last one left served.
  test.beforeEach(() => serve(snapshot()));

  test('standalone_first_run', async ({ page, baseURL }, testInfo) => {
    const origin = new URL(baseURL!).origin;
    const models = rows(document());
    const { requests, failed } = record(page);

    // R5.1, R5.3: one GET of /models.json; the page lists every size of the document in its order under its canonical
    // name, the browser variants in the tab, and opens on the browser default's row.
    await page.goto(PAGE);
    const picker = modelPicker(page);
    await expect(picker).toHaveValue(browserRow(MODEL));
    expect(libraryGets(requests)).toBe(1);
    await expect(picker.locator('optgroup[label="Pulled"]')).toHaveCount(0);
    await expect(picker.locator('optgroup[label="In browser · runs in this tab"] option')).toHaveText(
      models.filter((m) => m.browser).map((m) => `${m.name} · ${decimalSize(m.browser!.bytes)}`),
    );
    await expect(libraryRows(page)).toHaveText(models.map(row));
    const size = sized(document(), MODEL).browser!.bytes;
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs in this tab. The first run downloads ${decimalSize(size)}, which this browser keeps. ${IN_MEMORY}; only ` +
        'a reload frees all of it.',
    );
    await expect(page.getByTestId('models-fault')).toHaveCount(0);
    expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);

    // Its fonts, the engine module and onnxruntime-web (imported on the pick) come from under /playground/, and
    // nothing else is asked for: no /v1/ path, no other origin, no failed request.
    await page.evaluate(() => document.fonts.ready);
    const paths = () => requests.map((url) => new URL(url).pathname);
    await expect.poll(paths).toContain(`${PAGE}ort/1.30.0/ort.webgpu.bundle.min.mjs`);
    await expect.poll(paths).toContain(`${PAGE}engine/ardana-engine_bg.wasm`);
    expect(paths()).toContain(`${PAGE}fonts/onest-latin-wght-normal.woff2`);
    expect(paths()).toContain(`${PAGE}fonts/geist-mono-latin-wght-normal.woff2`);
    expect(paths()).toContain(`${PAGE}engine/engine.js`);
    for (const url of requests) {
      expect(new URL(url).origin, url).toBe(origin);
      expect(own(url), url).toBe(true);
    }
    expect(failed).toEqual([]);
    expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
    await screenshot(page, testInfo, 'standalone_first_run');
  });

  test('standalone_browser_run', async ({}, testInfo) => {
    test.setTimeout(900_000);
    expect(hub, 'xtask passes the stand-in of Hugging Face').toBeTruthy();
    const variant = sized(document(), MODEL).browser!;
    const files = ['tokenizer.json', 'model.onnx', 'model.onnx.data'].map(
      (file) => `${hub}/${variant.repo.replace(/^hf\.co\//, '')}/resolve/${variant.commit}/${file}`,
    );
    // The ardana CLI's own answer to the same request, from decider:0.8b's GGUF, offline over tmp/hf.
    const ticket = fixture('ticket.json');
    const home = testInfo.outputPath('home');
    const native = JSON.parse(
      execFileSync(process.env.ARDANA_BIN!, ['run', MODEL, '--request', '-', '--json'], {
        input: JSON.stringify(ticket),
        env: { ...process.env, ARDANA_HOME: home, HF_HUB_OFFLINE: '1' },
        encoding: 'utf8',
        timeout: 600_000,
      }),
    ) as Response;

    // A visitor whose browser keeps the files across visits: a persistent profile of its own, emptied first.
    const profile = testInfo.outputPath('profile');
    fs.rmSync(profile, { recursive: true, force: true });
    const context = await chromium.launchPersistentContext(profile, {
      channel: 'chrome',
      viewport: testInfo.project.use.viewport,
      baseURL: process.env.ARDANA_BASE_URL,
    });
    try {
      const page = context.pages()[0] ?? (await context.newPage());
      const { requests, failed } = record(page);
      const referers: string[] = [];
      page.on('request', (r) => {
        if (r.url().startsWith(hub)) {
          referers.push(r.headers().referer ?? '');
        }
      });
      await page.goto(PAGE);
      await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
      await loadPreset(page, 'Ticket routing');

      // R5.4: Run downloads the three files of `browser.repo` from the stand-in at the document's commit, and answers
      // in the tab with the document's profile as the CLI does.
      await runKey(page).click();
      await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
      await expect(page.getByTestId('fault')).toHaveCount(0);
      const answered = await tabResponse(page);
      expect(answered.model).toBe(native.model);
      expect(topAnswers(answered)).toEqual(topAnswers(native));
      await expectFiguresMatch(page, answered as unknown as Json);
      const fetched = requests.filter((url) => url.startsWith(hub));
      expect(fetched).toEqual(files);
      // huggingface.co refuses some referring pages (any on *.workers.dev), and so does the stand-in: none is named.
      expect(referers).toEqual(files.map(() => ''));
      expect(requests.filter((url) => new URL(url).pathname.startsWith('/v1/'))).toEqual([]);
      expect(failed).toEqual([]);
      await screenshot(page, testInfo, 'standalone_browser_run', 'answered');

      // After a reload this browser has the files: the run downloads under 1 MB from the stand-in, to the same answers.
      const sizes: Promise<number>[] = [];
      const fromHub = (response: { url(): string; request(): { sizes(): Promise<{ responseBodySize: number }> } }) => {
        if (response.url().startsWith(hub)) {
          sizes.push(response.request().sizes().then((s) => s.responseBodySize));
        }
      };
      page.on('response', fromHub);
      await page.reload();
      await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
      await loadPreset(page, 'Ticket routing');
      await expect(page.getByTestId('run-note')).toBeHidden();
      await runKey(page).click();
      await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
      const downloaded = (await Promise.all(sizes)).reduce((sum, bytes) => sum + bytes, 0);
      page.off('response', fromHub);
      expect(downloaded).toBeLessThan(1_000_000);
      expect(topAnswers(await tabResponse(page))).toEqual(topAnswers(native));
      console.log(
        `standalone_browser_run ${testInfo.project.name}: after a reload ${downloaded} bytes from the hub; ` +
          `top answers ${JSON.stringify(topAnswers(native))}`,
      );
      await screenshot(page, testInfo, 'standalone_browser_run');
    } finally {
      await context.close();
    }
  });

  test('standalone_run_command', async ({ page }, testInfo) => {
    const models = rows(document());
    const plain = models.filter((m) => !m.browser);
    expect(plain.length).toBeGreaterThan(0);
    const { requests } = record(page);
    const ticket = fixture('ticket.json');

    // R5.5: a library size with no browser variant shows the install line and the CLI's command under its canonical
    // name; Run sends nothing.
    await page.goto(PAGE);
    await loadPreset(page, 'Ticket routing');
    for (const model of plain) {
      await pickModel(page, model.name);
      await expectInstall(page, model.name, ticket, requests);
      await expect(page.getByTestId('model-note')).toHaveText(
        `Runs with the ardana CLI on your machine; its first run downloads ${decimalSize(model.bytes)}.`,
      );
    }
    await screenshot(page, testInfo, 'standalone_run_command', 'library');

    // A share link naming a model the library lacks, with and without `?autorun=1`: the same, the model as named.
    const unknown = 'speed_latest';
    for (const autorun of ['', '?autorun=1']) {
      const sent = requests.length;
      await page.goto(`${PAGE}${autorun}${shareHash({ ...ticket, model: unknown })}`);
      await expectInstall(page, unknown, ticket, requests);
      await expect(page.getByTestId('model-note')).toHaveText('Runs with the ardana CLI on your machine.');
      // Loading the page fetched its own files and the library document alone.
      for (const url of requests.slice(sent)) {
        expect(own(url), url).toBe(true);
      }
    }
    await screenshot(page, testInfo, 'standalone_run_command', 'unknown');

    // The banner's switch picks the browser default's "In browser" row and gives Run the focus, described by what its
    // first run downloads; nothing is downloaded before the tap.
    await page.getByTestId('run-note').getByRole('button', { name: `Run ${MODEL} in this tab instead` }).click();
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(runKey(page)).toBeFocused();
    const size = sized(document(), MODEL).browser!.bytes;
    await expect(runKey(page)).toHaveAccessibleDescription(
      startsWith(`Run downloads ${MODEL} into this tab once: ${decimalSize(size)}, kept by this browser.`),
    );
    expect(requests.filter((url) => url.startsWith(hub) || new URL(url).pathname.startsWith('/v1/'))).toEqual([]);
    await screenshot(page, testInfo, 'standalone_run_command');
  });

  test('standalone_share', async ({ page, browser, baseURL }, testInfo) => {
    const origin = new URL(baseURL!).origin;
    await page.goto(PAGE);
    await loadPreset(page, 'Support-chat audit');
    await pickModel(page, 'decider:4b');
    const state = await stateBox(page).inputValue();
    const questions = await questionsBox(page).inputValue();

    // R5.5: the link is the page's own, under /playground/, and opens the same editors and canonical name.
    await page.getByRole('button', { name: 'Share link' }).click();
    const link = await page.getByRole('textbox', { name: 'Link to these inputs' }).inputValue();
    expect(link.startsWith(`${origin}${PAGE}#share/`), link).toBe(true);
    await screenshot(page, testInfo, 'standalone_share', 'link');

    const context = await browser.newContext({ viewport: testInfo.project.use.viewport });
    const fresh = await context.newPage();
    await fresh.goto(link);
    await expect(stateBox(fresh)).toHaveValue(state);
    await expect(questionsBox(fresh)).toHaveValue(questions);
    await expect(modelPicker(fresh)).toHaveValue('decider:4b');
    await expect(fresh.getByTestId('channel')).toHaveCount(Object.keys(JSON.parse(questions)).length);
    await screenshot(fresh, testInfo, 'standalone_share');
    await context.close();
  });

  // R5.3: a size published in a family of the document after the page loaded (no rebuild of this page, no reload of
  // the tab) is in the picker when the tab comes back, in the document's order: after its family's other sizes and
  // before the next family's, under its canonical name; it runs as any library row does.
  test('standalone_library_update', async ({ page }, testInfo) => {
    const library = document();
    const { requests, failed } = record(page);
    await page.goto(PAGE);
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(libraryRows(page)).toHaveText(rows(library).map(row));
    expect(libraryGets(requests)).toBe(1);
    await page.evaluate(() => ((window as unknown as { kept?: string }).kept = 'kept'));

    const decider = library.models.find((f) => f.name === 'decider')!;
    const four = decider.sizes.find((s) => s.size === '4b')!;
    const added: Size = {
      ...four,
      size: '9b',
      params: '9B',
      gguf: {
        ...four.gguf,
        repo: 'hf.co/ardana-ai/decider-9b-GGUF',
        quants: [{ quant: four.gguf.default, file: 'decider-9b-Q4_K_M.gguf', bytes: 9_000_000_000 }],
      },
    };
    const updated: Document = {
      ...library,
      models: library.models.map((f) => (f === decider ? { ...f, sizes: [...f.sizes, added] } : f)),
    };
    serve(updated);
    await comeBack(page, 'focus');
    const listed = rows(updated);
    expect(listed.map((r) => r.name).slice(0, 4)).toEqual(['decider:0.8b', 'decider:2b', 'decider:4b', 'decider:9b']);
    await expect(libraryRows(page)).toHaveText(listed.map(row));
    expect(libraryGets(requests)).toBe(2);
    // No reload: the page kept what its script held, and the pick.
    expect(await page.evaluate(() => (window as unknown as { kept?: string }).kept)).toBe('kept');
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(page.getByTestId('models-fault')).toHaveCount(0);
    await pickModel(page, 'decider:9b');
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs with the ardana CLI on your machine; its first run downloads ${decimalSize(9_000_000_000)}.`,
    );
    expect(failed).toEqual([]);
    await screenshot(page, testInfo, 'standalone_library_update');
  });

  // R5.2: the document's GET failing before the page loads, and again while it is open. The page says the library is
  // unavailable (under the picker, an alert; in the banner while nothing is picked yet, where Run is held), keeps its
  // list, pick and editors, and lists the models after the next successful GET on the tab's return; a family and a
  // size for a later ardana, and a family and a size that do not read, are left out (Q13); another schema is
  // unavailable.
  test('standalone_library_unavailable', async ({ page }, testInfo) => {
    const library = document();
    const { requests } = record(page);
    const unavailable = 'The model library is unavailable';
    const fault = page.getByTestId('models-fault');

    // Gone before the load: nothing to list, Run held, the editors at work.
    fs.rmSync(served);
    await page.goto(PAGE);
    await expect(fault).toHaveText(`${unavailable}: GET ${LIBRARY} answered 404`);
    await expect(fault).toHaveAttribute('role', 'alert');
    await expect(modelPicker(page).locator('option')).toHaveCount(0);
    await expect(runKey(page)).toHaveAttribute('aria-disabled', 'true');
    await expect(page.getByTestId('run-note')).toHaveText(unavailable);
    await expect(runKey(page)).toHaveAccessibleDescription(unavailable);
    await loadPreset(page, 'Ticket routing');
    const state = await stateBox(page).inputValue();
    expect(state.length).toBeGreaterThan(0);
    expect(libraryGets(requests)).toBe(1);
    await screenshot(page, testInfo, 'standalone_library_unavailable', 'first-load');

    // Back, with a family and a size for a later ardana and a family and a size that do not read: the tab's return
    // lists the sizes the page reads, in the document's order, opens on the browser default's row and frees Run; the
    // editors keep their text.
    const [decider, ...others] = library.models;
    const laterSize: Size = { ...decider.sizes[1], size: '8b', min_version: '99.0.0' };
    const laterFamily: Family = { ...others[0], name: 'later', min_version: '99.0.0' };
    const brokenSize = { size: '9b', params: '9B' } as unknown as Size;
    const brokenFamily = { name: 'broken', sizes: decider.sizes } as unknown as Family;
    serve({
      ...library,
      models: [
        { ...decider, sizes: [decider.sizes[0], laterSize, ...decider.sizes.slice(1), brokenSize] },
        laterFamily,
        ...others,
        brokenFamily,
      ],
    });
    await comeBack(page, 'focus');
    await expect(fault).toHaveCount(0);
    await expect(libraryRows(page)).toHaveText(rows(library).map(row));
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
    await expect(page.getByTestId('run-note')).not.toContainText('library');
    await expect(stateBox(page)).toHaveValue(state);
    expect(libraryGets(requests)).toBe(2);

    // Gone again while the page is open: the tab's return says so, and the page keeps its list, its pick and its
    // editors; Run is Run.
    fs.rmSync(served);
    await comeBack(page, 'visibilitychange');
    await expect(fault).toHaveText(`${unavailable}: GET ${LIBRARY} answered 404`);
    await expect(libraryRows(page)).toHaveText(rows(library).map(row));
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
    await expect(stateBox(page)).toHaveValue(state);
    await screenshot(page, testInfo, 'standalone_library_unavailable');

    // A document of another schema is unavailable too, and said with its reason; the next good one lists again.
    serve({ ...library, schema: 2 });
    await comeBack(page, 'focus');
    await expect(fault).toHaveText(`${unavailable}: the library document is schema 2, which this ardana does not read; update ardana`);
    await expect(libraryRows(page)).toHaveText(rows(library).map(row));
    serve(library);
    await comeBack(page, 'visibilitychange');
    await expect(fault).toHaveCount(0);
    await expect(libraryRows(page)).toHaveText(rows(library).map(row));
    expect(libraryGets(requests)).toBe(5);
  });
});
