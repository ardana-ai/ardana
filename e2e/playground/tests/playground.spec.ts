// W6 cases of the embedded playground, against the release `ardana` serving decider-2b (see playwright.config.ts).
import { expect, test } from '@playwright/test';
import {
  expectFiguresMatch,
  fixture,
  isSystemOne,
  screenshot,
  shareHash,
  type Json,
  type Request,
} from './helpers';

const runKey = (page: import('@playwright/test').Page) => page.getByRole('button', { name: /^Run/ });

test('embedded_binary', async ({ page }, testInfo) => {
  const index = await page.request.get('/');
  expect(index.status()).toBe(200);
  expect(index.headers()['content-type']).toContain('text/html');
  const html = await index.text();

  // Every asset index.html names is served, the wasm with its own type.
  const assets = [...html.matchAll(/(?:href|src|module_or_path:)\s*=?\s*['"](\/[^'"]+)['"]/g)].map((m) => m[1]);
  expect(assets.some((a) => a.endsWith('.wasm'))).toBe(true);
  expect(assets.some((a) => a.endsWith('.css'))).toBe(true);
  for (const asset of new Set(assets)) {
    const reply = await page.request.get(asset);
    expect(reply.status(), asset).toBe(200);
    if (asset.endsWith('.wasm')) {
      expect(reply.headers()['content-type']).toBe('application/wasm');
    }
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
  await expect(runKey(page)).toBeEnabled();
  await screenshot(page, testInfo, 'embedded_binary');
});

test('answers_match_api', async ({ page }, testInfo) => {
  const ticket = fixture('ticket.json');
  // Load the model first, so the timed run measures a decision, not a load.
  expect((await page.request.post('/v1/systemone', { data: ticket })).status()).toBe(200);

  await page.goto(`/${shareHash(ticket)}`);
  await expect(page.getByRole('textbox', { name: 'State' })).toHaveValue(ticket.state as string);
  await expect(page.getByTestId('channel')).toHaveCount(Object.keys(ticket.questions).length);
  await expect(page.locator('#model')).toHaveValue('jev-latest');
  await screenshot(page, testInfo, 'answers_match_api', 'loaded');

  const replied = page.waitForResponse((r) => isSystemOne(r.url()));
  const finished = page.waitForEvent('requestfinished', (r) => isSystemOne(r.url()));
  await runKey(page).click();
  const response = await replied;
  expect(response.status()).toBe(200);
  const json = (await response.json()) as { answers: Record<string, Record<string, Json>>; usage: Record<string, number> };
  const timing = (await finished).timing();

  await expect(page.getByTestId('fault')).toHaveCount(0);
  for (const [id, answer] of Object.entries(json.answers)) {
    const channel = page.locator(`[data-testid="channel"][data-question="${id}"]`);
    await expect(channel.getByTestId('type')).toContainText(`Type ${answer.type}`);
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
  for (const [id, answer] of Object.entries(json.answers)) {
    const expected =
      answer.type === 'noul'
        ? [`/answers/${id}/noul`]
        : [
            `/answers/${id}/confidence`,
            ...Object.keys(answer.probabilities as Record<string, number>).map((o) => `/answers/${id}/probabilities/${o}`),
          ];
    for (const field of expected) {
      expect(fields, `${field} is displayed`).toContain(field);
    }
  }
  await expect(page.getByTestId('answered-by').locator('[data-field="/model"]')).toHaveAttribute(
    'data-value',
    (json as unknown as { model: string }).model,
  );
  expect(fields).toContain('/model');
  await expect(page.getByTestId('tokens-in')).toContainText(String(json.usage.input_tokens));
  await expect(page.getByTestId('tokens-out')).toContainText(String(json.usage.output_tokens));
  await expect(page.getByTestId('cassette-tokens')).toContainText(String(json.usage.input_tokens));

  expect(timing.responseEnd, 'Playwright timed the request').toBeGreaterThan(0);
  const shown = Number(await page.getByTestId('latency').locator('.matrix-digits').textContent());
  expect(Math.abs(shown - timing.responseEnd), `displayed ${shown} ms, measured ${timing.responseEnd} ms`).toBeLessThanOrEqual(20);

  await screenshot(page, testInfo, 'answers_match_api');
});

test('picker_raw_errors', async ({ page, baseURL }, testInfo) => {
  const origin = new URL(baseURL!).origin;
  const requests: { url: string; type: string }[] = [];
  page.on('request', (r) => requests.push({ url: r.url(), type: r.resourceType() }));

  // The picker lists /v1/models, after the default model alias.
  const models = (await (await page.request.get('/v1/models')).json()) as { models: { name: string }[] };
  const names = models.models.map((m) => m.name);
  expect(names.length).toBeGreaterThan(0);
  const ticket = fixture('ticket.json');
  await page.goto(`/${shareHash(ticket)}`);
  const picker = page.getByRole('combobox', { name: 'Model' });
  await expect(picker.locator('option')).toHaveCount(names.length + 1);
  expect(await picker.locator('option').evaluateAll((os) => os.map((o) => (o as HTMLOptionElement).value))).toEqual([
    'jev-latest',
    ...names,
  ]);

  // The raw panel shows exactly what was sent and received.
  await picker.selectOption(names[0]);
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
