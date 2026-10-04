// W6 cases of the embedded playground, against the release `ardana` serving decider-2b (see playwright.config.ts).
// The share links here are written as Jev's playground writes them, with its `jev-latest` alias; the picker reads
// the alias as the server's default model and names that model.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  browserRow,
  decimalSize,
  expectFiguresMatch,
  fixture,
  isSystemOne,
  pickInBrowser,
  pickModel,
  picker as modelPicker,
  root,
  runKey,
  screenshot,
  shareHash,
  type Json,
  type ModelInfo,
  type Request,
} from './helpers';

/** Q13: every response keeps the page cross-origin isolated and carries no CORS header. */
function isolated(what: string, headers: Record<string, string>) {
  expect(headers['cross-origin-opener-policy'], what).toBe('same-origin');
  expect(headers['cross-origin-embedder-policy'], what).toBe('require-corp');
  expect(headers['cross-origin-resource-policy'], what).toBe('same-origin');
  expect(Object.keys(headers).filter((name) => name.startsWith('access-control-')), what).toEqual([]);
}

test('embedded_binary', async ({ page, baseURL }, testInfo) => {
  const origin = new URL(baseURL!).origin;
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));
  const index = await page.request.get('/');
  expect(index.status()).toBe(200);
  expect(index.headers()['content-type']).toContain('text/html');
  isolated('/', index.headers());
  const html = await index.text();

  // Every asset index.html names, by an absolute path or one relative to the page, is served, the wasm with its own
  // type.
  const assets = [...html.matchAll(/(?:href|src|module_or_path:)\s*=?\s*['"]([^'"#:]+)['"]/g)].map(
    (m) => new URL(m[1], 'http://page/').pathname,
  );
  expect(assets).toContain('/favicon.svg');
  expect(assets).toContain('/engine/engine.js');
  expect(assets.some((a) => a.endsWith('.wasm'))).toBe(true);
  expect(assets.some((a) => a.endsWith('.css'))).toBe(true);
  for (const asset of new Set(assets)) {
    const reply = await page.request.get(asset);
    expect(reply.status(), asset).toBe(200);
    isolated(asset, reply.headers());
    if (asset.endsWith('.wasm')) {
      expect(reply.headers()['content-type']).toBe('application/wasm');
    }
  }

  // R2.6: onnxruntime-web 1.30.0 comes from the page's own origin, byte for byte the vendored files.
  const vendored = path.join(root, 'crates/ardana-playground/ort/1.30.0');
  for (const [file, type] of [
    ['ort.webgpu.bundle.min.mjs', /^(application|text)\/javascript/],
    ['ort-wasm-simd-threaded.asyncify.wasm', /^application\/wasm$/],
    ['ort.wasm.bundle.min.mjs', /^(application|text)\/javascript/],
    ['ort-wasm-simd-threaded.wasm', /^application\/wasm$/],
  ] as const) {
    const reply = await page.request.get(`/ort/1.30.0/${file}`);
    expect(reply.status(), file).toBe(200);
    expect(reply.headers()['content-type'], file).toMatch(type);
    isolated(file, reply.headers());
    expect(Buffer.compare(await reply.body(), fs.readFileSync(path.join(vendored, file))), file).toBe(0);
  }

  // Unknown paths get the SPA's index.html; unknown API paths keep the API's 404.
  for (const deep of ['/deck/some/where', '/share']) {
    const reply = await page.request.get(deep);
    expect(reply.status(), deep).toBe(200);
    expect(await reply.text()).toBe(html);
  }
  const apiMiss = await page.request.get('/v1/nope');
  expect(apiMiss.status()).toBe(404);
  expect(await apiMiss.json()).toEqual({ detail: 'Not Found' });

  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1, name: 'Ardana' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'State' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Questions JSON' })).toBeVisible();
  // Run is held while there is nothing to run, and says why.
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'true');
  await expect(page.getByTestId('run-note')).toHaveText('Add a question to run');
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);

  // Picking an "In browser" row loads the runtime from this origin; the module the page loaded is 1.30.0.
  const loaded = page.waitForResponse((r) => new URL(r.url()).pathname === '/ort/1.30.0/ort.webgpu.bundle.min.mjs');
  await pickInBrowser(page, 'decider-0.8b');
  expect((await loaded).status()).toBe(200);
  expect(await page.evaluate("import('/ort/1.30.0/ort.webgpu.bundle.min.mjs').then((ort) => ort.env.versions.web)")).toBe(
    '1.30.0',
  );
  expect(requests.length).toBeGreaterThan(0);
  for (const url of requests) {
    expect(new URL(url).origin, url).toBe(origin);
  }
  // The embedded build lists /v1/models; the library document is the standalone build's alone (Q13).
  expect(requests.map((url) => new URL(url).pathname)).toContain('/v1/models');
  expect(requests.map((url) => new URL(url).pathname)).not.toContain('/models.json');
  await screenshot(page, testInfo, 'embedded_binary');
});

test('answers_match_api', async ({ page }, testInfo) => {
  const ticket = fixture('ticket.json');
  // Load the model first, so the timed run measures a decision, not a load.
  expect((await page.request.post('/v1/systemone', { data: ticket })).status()).toBe(200);

  await page.goto(`/${shareHash(ticket)}`);
  await expect(page.getByRole('textbox', { name: 'State' })).toHaveValue(ticket.state as string);
  await expect(page.getByTestId('channel')).toHaveCount(Object.keys(ticket.questions).length);
  const models = (await (await page.request.get('/v1/models')).json()) as { models: ModelInfo[] };
  const defaultModel = models.models.find((m) => m.x_default)!.name;
  expect(defaultModel).toBe('decider-2b');
  await expect(page.locator('#model')).toHaveValue(defaultModel);
  await expect(page.locator(`#model option[value="${defaultModel}"]`)).toHaveText(defaultModel);
  await screenshot(page, testInfo, 'answers_match_api', 'loaded');

  const replied = page.waitForResponse((r) => isSystemOne(r.url()));
  const finished = page.waitForEvent('requestfinished', (r) => isSystemOne(r.url()));
  await runKey(page).click();
  const response = await replied;
  expect(response.status()).toBe(200);
  const json = (await response.json()) as { answers: Record<string, Record<string, Json>>; usage: Record<string, number> };
  const timing = (await finished).timing();

  await expect(page.getByTestId('fault')).toHaveCount(0);
  expect(response.request().postDataJSON().model).toBe(defaultModel);
  // Focus stays on RUN, or on a phone, where the answers land a screen below it, goes to the first question's id; the
  // polite status region reads the outcome.
  if (testInfo.project.name === 'mobile') {
    await expect(page.getByTestId('channel').first().getByRole('heading', { level: 3 })).toBeFocused();
  } else {
    await expect(runKey(page)).toBeFocused();
  }
  await expect(page.getByTestId('run-status')).toHaveText(
    new RegExp(`^Answered by ${(json as unknown as { model: string }).model}: 2 questions, \\d+ ms$`),
  );
  for (const [id, answer] of Object.entries(json.answers)) {
    const channel = page.locator(`[data-testid="channel"][data-question="${id}"]`);
    await expect(channel.getByTestId('type').getByRole('radio', { name: answer.type as string, exact: true })).toBeChecked();
    if (answer.type === 'choice' || answer.type === 'score') {
      const options = Object.keys(answer.probabilities as Record<string, number>);
      await expect(channel.getByTestId('ladder')).toHaveCount(options.length);
    } else {
      await expect(channel.getByTestId('ladder')).toHaveCount(1);
    }
    if (answer.type === 'choice') {
      await expect(channel.locator('[data-testid="ladder"].lit')).toHaveCount(1);
      await expect(channel.locator('[data-testid="ladder"].lit')).toContainText(answer.choice as string);
    }
  }

  const fields = await expectFiguresMatch(page, json as unknown as Json);
  // R6.5's display formats, by the `data-format` each figure declares: probabilities as percent with one decimal
  // (`percent`), confidence, noul and score with two decimals (`fixed2`).
  for (const [id, answer] of Object.entries(json.answers)) {
    const expected: [string, string][] =
      answer.type === 'noul'
        ? [[`/answers/${id}/noul`, 'fixed2']]
        : [
            ...(answer.type === 'score' ? [[`/answers/${id}/score`, 'fixed2'] as [string, string]] : []),
            [`/answers/${id}/confidence`, 'fixed2'],
            ...Object.keys(answer.probabilities as Record<string, number>).map(
              (o): [string, string] => [`/answers/${id}/probabilities/${o}`, 'percent'],
            ),
          ];
    for (const [field, format] of expected) {
      expect(fields, `${field} is displayed`).toContain(field);
      await expect(page.locator(`[data-field=${JSON.stringify(field)}]`), `${field} format`).toHaveAttribute(
        'data-format',
        format,
      );
    }
  }
  await expect(page.getByTestId('answered-by').locator('[data-field="/model"]')).toHaveAttribute(
    'data-value',
    (json as unknown as { model: string }).model,
  );
  expect(fields).toContain('/model');
  await expect(page.getByTestId('tokens-in')).toContainText(String(json.usage.input_tokens));
  await expect(page.getByTestId('tokens-out')).toContainText(String(json.usage.output_tokens));
  await expect(page.getByTestId('state-tokens')).toContainText(String(json.usage.input_tokens));

  expect(timing.responseEnd, 'Playwright timed the request').toBeGreaterThan(0);
  const shown = Number(await page.getByTestId('latency').locator('[data-ms]').textContent());
  expect(Math.abs(shown - timing.responseEnd), `displayed ${shown} ms, measured ${timing.responseEnd} ms`).toBeLessThanOrEqual(20);

  await screenshot(page, testInfo, 'answers_match_api');
});

test('picker_raw_errors', async ({ page, baseURL }, testInfo) => {
  const origin = new URL(baseURL!).origin;
  const requests: { url: string; type: string }[] = [];
  page.on('request', (r) => requests.push({ url: r.url(), type: r.resourceType() }));

  // The picker lists /v1/models: the pulled models, then the models whose browser variant runs in the tab (a pulled
  // one too: each row runs where it says), then the library models this server has not pulled, which the ardana CLI
  // runs, each with its download size.
  const models = (await (await page.request.get('/v1/models')).json()) as { models: ModelInfo[] };
  const names = models.models.map((m) => m.name);
  const pulled = models.models.filter((m) => m.x_pulled !== false);
  const inBrowser = models.models.filter((m) => m.x_browser !== undefined);
  const library = models.models.filter((m) => m.x_pulled === false);
  expect(pulled.map((m) => m.name)).toEqual(['decider-2b']);
  expect(inBrowser.map((m) => m.name)).toEqual(['decider-2b', 'decider-0.8b', 'qwen3.5-0.8b']);
  expect(library.length).toBeGreaterThan(0);
  const ticket = fixture('ticket.json');
  await page.goto(`/${shareHash(ticket)}`);
  const picker = modelPicker(page);
  const values = [...pulled.map((m) => m.name), ...inBrowser.map((m) => browserRow(m.name)), ...library.map((m) => m.name)];
  await expect(picker.locator('option')).toHaveCount(values.length);
  expect(await picker.locator('option').evaluateAll((os) => os.map((o) => (o as HTMLOptionElement).value))).toEqual(
    values,
  );
  await expect(picker.locator('optgroup[label="Pulled"] option')).toHaveText(pulled.map((m) => m.name));
  await expect(picker.locator('optgroup[label="In browser · runs in this tab"] option')).toHaveText(
    inBrowser.map((m) => `${m.name} · ${decimalSize(m.x_browser!)}`),
  );
  await expect(picker.locator('optgroup[label="Library · runs with the ardana CLI"] option')).toHaveText(
    library.map((m) => `${m.name} · ${decimalSize(m.x_size!)}`),
  );

  // The raw panel shows exactly what was sent and received.
  await pickModel(page, names[0]);
  let replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await runKey(page).click();
  let response = await replied;
  expect(response.status()).toBe(200);
  expect(response.request().postDataJSON().model).toBe(names[0]);
  await expect(page.getByTestId('raw-request')).toHaveJSProperty('textContent', response.request().postData()!);
  await expect(page.getByTestId('raw-response')).toHaveJSProperty('textContent', await response.text());
  await expect(page.getByTestId('raw-status')).toHaveText('HTTP 200');

  // 413: a state over the context window, with the API's detail.
  await page.getByRole('textbox', { name: 'State' }).fill('refund please '.repeat(30_000));
  replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await runKey(page).click();
  response = await replied;
  expect(response.status()).toBe(413);
  const tooLarge = (await response.json()) as { detail: { error_type: string; message: string } };
  await expect(page.getByTestId('fault')).toContainText('HTTP 413');
  await expect(page.getByTestId('fault-message')).toHaveText(tooLarge.detail.message);
  await expect(page.getByTestId('raw-status')).toHaveText('HTTP 413');
  await expect(page.getByTestId('run-status')).toHaveText('HTTP 413, not answered');
  await screenshot(page, testInfo, 'picker_raw_errors', '413');

  // An answer type the page does not know renders as raw JSON. The server only returns the three Jev types, so
  // this step (and only this step) rewrites one answer's type in the server's real response.
  await page.getByRole('textbox', { name: 'State' }).fill(ticket.state as string);
  let rewritten: Json = null;
  await page.route(
    (url) => isSystemOne(url.href),
    async (route) => {
      const real = await route.fetch();
      const json = await real.json();
      json.answers.refund.type = 'rank';
      rewritten = json.answers.refund;
      await route.fulfill({ response: real, json });
    },
  );
  replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await runKey(page).click();
  await replied;
  const raw = page.locator('[data-question="refund"]').getByTestId('raw-answer');
  await expect(raw).toBeVisible();
  expect(JSON.parse((await raw.textContent())!)).toEqual(rewritten);
  await page.unrouteAll();
  await screenshot(page, testInfo, 'picker_raw_errors', 'unknown-type');

  // 422: a share link whose request is invalid, run on load; every detail entry is shown.
  const invalid: Request = structuredClone(ticket);
  (invalid.questions.department as Record<string, Json>).criteria = ['billing'];
  replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await page.goto(`/?autorun=1${shareHash(invalid)}`);
  response = await replied;
  expect(response.status()).toBe(422);
  const rejected = (await response.json()) as { detail: { loc: (string | number)[]; msg: string }[] };
  await expect(page.getByTestId('fault-issue')).toHaveCount(rejected.detail.length);
  for (const [i, issue] of rejected.detail.entries()) {
    await expect(page.getByTestId('fault-issue').nth(i)).toContainText(issue.msg);
    await expect(page.getByTestId('fault-issue').nth(i)).toContainText(issue.loc.join(' › '));
  }
  await expect(page.getByTestId('raw-status')).toHaveText('HTTP 422');
  await expect(page.getByTestId('raw-response')).toHaveJSProperty('textContent', await response.text());
  // Each issue that names a question marks that channel with its message.
  const named = rejected.detail.filter((issue) => issue.loc[0] === 'body' && issue.loc[1] === 'questions');
  expect(named.length).toBeGreaterThan(0);
  for (const issue of named) {
    const faulted = page.locator(`[data-testid="channel"][data-question="${issue.loc[2]}"]`);
    await expect(faulted).toHaveClass(/faulted/);
    await expect(faulted.getByTestId('channel-fault')).toContainText(issue.msg);
  }
  await expect(page.locator('[data-testid="channel"].faulted')).toHaveCount(new Set(named.map((i) => i.loc[2])).size);

  // Every request the page made stayed on its origin, and every call it made beyond loading its own wasm went to
  // /v1/*.
  expect(requests.length).toBeGreaterThan(0);
  for (const { url, type } of requests) {
    expect(new URL(url).origin, url).toBe(origin);
    if ((type === 'fetch' || type === 'xhr') && !new URL(url).pathname.endsWith('.wasm')) {
      expect(url.startsWith(`${origin}/v1/`), url).toBe(true);
    }
  }
  await screenshot(page, testInfo, 'picker_raw_errors');
});
