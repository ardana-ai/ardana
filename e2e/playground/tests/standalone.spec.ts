// W2, the standalone playground (`cargo xtask e2e standalone`, tagged @standalone): the build of
// `cargo xtask build-playground --public-url /playground/ --hub <stand-in>` as static files under `/playground/`
// (`ARDANA_BASE_URL`), with no server behind it, and a stand-in of Hugging Face on another origin serving `tmp/hf`
// offline (`ARDANA_HUB_URL`). The page lists the library it baked, runs the browser variants in the tab from the
// stand-in's ardana-ai repositories at the baked commits, and hands every other model to the ardana CLI.
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
  type ModelInfo,
  type Request,
} from './helpers';

const MODEL = 'decider-0.8b';
const PAGE = '/playground/';
const hub = process.env.ARDANA_HUB_URL!;

/** What `cargo xtask build-playground` baked into the page: the library's models and its browser variants. */
type Library = {
  hub: string;
  models: { models: ModelInfo[] };
  browser: Record<string, { repository: string; commit: string }>;
};
const library = (): Library => JSON.parse(fs.readFileSync(path.join(root, 'tmp/playground/library.json'), 'utf8'));

/** The names of the models `library.toml` lists, in its order. */
const libraryNames = () =>
  [...fs.readFileSync(path.join(root, 'crates/ardana-registry/src/library.toml'), 'utf8').matchAll(/^name = "(.+)"$/gm)].map(
    (match) => match[1],
  );

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
  test('standalone_first_run', async ({ page, baseURL }, testInfo) => {
    const origin = new URL(baseURL!).origin;
    const { models, browser } = library();
    expect(models.models.map((m) => m.name)).toEqual(libraryNames());
    const { requests, failed } = record(page);

    // R2.2: the page lists every library model, the browser variants in the tab, and opens on decider-0.8b's row.
    await page.goto(PAGE);
    const picker = modelPicker(page);
    await expect(picker).toHaveValue(browserRow(MODEL));
    await expect(picker.locator('optgroup[label="Pulled"]')).toHaveCount(0);
    await expect(picker.locator('optgroup[label="In browser · runs in this tab"] option')).toHaveText(
      models.models.filter((m) => m.x_browser !== undefined).map((m) => `${m.name} · ${decimalSize(m.x_browser!)}`),
    );
    expect(Object.keys(browser).sort()).toEqual(
      models.models.filter((m) => m.x_browser !== undefined).map((m) => m.name).sort(),
    );
    await expect(picker.locator('optgroup[label="Library · runs with the ardana CLI"] option')).toHaveText(
      models.models.map((m) => `${m.name} · ${decimalSize(m.x_size!)}`),
    );
    const size = models.models.find((m) => m.name === MODEL)!.x_browser!;
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs in this tab. The first run downloads ${decimalSize(size)}, which this browser keeps. ${IN_MEMORY}; only ` +
        'a reload frees all of it.',
    );
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
      expect(new URL(url).pathname.startsWith(PAGE), url).toBe(true);
    }
    expect(failed).toEqual([]);
    expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
    await screenshot(page, testInfo, 'standalone_first_run');
  });

  test('standalone_browser_run', async ({}, testInfo) => {
    test.setTimeout(900_000);
    expect(hub, 'xtask passes the stand-in of Hugging Face').toBeTruthy();
    const variant = library().browser[MODEL];
    const files = ['tokenizer.json', 'model.onnx', 'model.onnx.data'].map(
      (file) => `${hub}/${variant.repository}/resolve/${variant.commit}/${file}`,
    );
    // The ardana CLI's own answer to the same request, from decider-0.8b's GGUF, offline over tmp/hf.
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
      await page.goto(PAGE);
      await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
      await loadPreset(page, 'Ticket routing');

      // R2.3: Run downloads the three files from the stand-in at the baked commit, and answers in the tab as the CLI
      // does.
      await runKey(page).click();
      await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
      await expect(page.getByTestId('fault')).toHaveCount(0);
      const answered = await tabResponse(page);
      expect(answered.model).toBe(native.model);
      expect(topAnswers(answered)).toEqual(topAnswers(native));
      await expectFiguresMatch(page, answered as unknown as Json);
      const fetched = requests.filter((url) => url.startsWith(hub));
      expect(fetched).toEqual(files);
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
    const { models } = library();
    const plain = models.models.filter((m) => m.x_browser === undefined).map((m) => m.name);
    expect(plain.length).toBeGreaterThan(0);
    const { requests } = record(page);
    const ticket = fixture('ticket.json');

    // R2.4: a library model with no browser variant shows the install line and the CLI's command; Run sends nothing.
    await page.goto(PAGE);
    await loadPreset(page, 'Ticket routing');
    for (const model of plain) {
      await pickModel(page, model);
      await expectInstall(page, model, ticket, requests);
      const size = decimalSize(models.models.find((m) => m.name === model)!.x_size!);
      await expect(page.getByTestId('model-note')).toHaveText(
        `Runs with the ardana CLI on your machine; its first run downloads ${size}.`,
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
      // Loading the page fetched its own files alone.
      for (const url of requests.slice(sent)) {
        expect(new URL(url).pathname.startsWith(PAGE), url).toBe(true);
      }
    }
    await screenshot(page, testInfo, 'standalone_run_command', 'unknown');

    // The banner's switch picks the browser default's "In browser" row and gives Run the focus, described by what its
    // first run downloads; nothing is downloaded before the tap.
    await page.getByTestId('run-note').getByRole('button', { name: `Run ${MODEL} in this tab instead` }).click();
    await expect(modelPicker(page)).toHaveValue(browserRow(MODEL));
    await expect(runKey(page)).toBeFocused();
    const size = models.models.find((m) => m.name === MODEL)!.x_browser!;
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
    await pickModel(page, 'decider-4b');
    const state = await stateBox(page).inputValue();
    const questions = await questionsBox(page).inputValue();

    // R2.5: the link is the page's own, under /playground/, and opens the same editors and model.
    await page.getByRole('button', { name: 'Share link' }).click();
    const link = await page.getByRole('textbox', { name: 'Link to these inputs' }).inputValue();
    expect(link.startsWith(`${origin}${PAGE}#share/`), link).toBe(true);
    await screenshot(page, testInfo, 'standalone_share', 'link');

    const context = await browser.newContext({ viewport: testInfo.project.use.viewport });
    const fresh = await context.newPage();
    await fresh.goto(link);
    await expect(stateBox(fresh)).toHaveValue(state);
    await expect(questionsBox(fresh)).toHaveValue(questions);
    await expect(modelPicker(fresh)).toHaveValue('decider-4b');
    await expect(fresh.getByTestId('channel')).toHaveCount(Object.keys(JSON.parse(questions)).length);
    await screenshot(fresh, testInfo, 'standalone_share');
    await context.close();
  });
});
