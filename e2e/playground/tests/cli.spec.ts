// W3, the models a server cannot run, as the user amended Q2, Q4 and R3.3. On a local server, which ardana runs
// already, a model it has not pulled shows `ardana pull <name>` and `ardana run <name> --request -` with the exact
// request, never ardana.ai's install line, and once `ardana pull` has added it the page runs it on the server; on a
// public server, which runs no model, a server row shows the install line and `ardana run`, and offers the browser
// default in this tab instead. Run sends nothing for a model the server would have to pull, autorun included.
// `run_command` runs on the release `ardana serve` holding decider-2b, `pull_while_open` on an empty server of its own
// (the playground suite); `public_playground` (tagged @public, `cargo xtask e2e public`) on `ardana serve --public`.
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  INSTALL,
  ardanaCommand,
  browserRow,
  decimalSize,
  expectFiguresMatch,
  fixture,
  isSystemOne,
  loadPreset,
  pickInBrowser,
  pickModel,
  picker as modelPicker,
  run,
  runKey,
  screenshot,
  shareHash,
  visibleText,
  type Json,
  type ModelInfo,
  type Request,
} from './helpers';

/** The body Run sends for `model` while the editors hold `request`'s state and questions, as the page writes it. */
const body = (model: string, request: Request) =>
  JSON.stringify({ model, state: request.state, questions: request.questions }, null, 2);

/** `text` as a regular expression that matches it from its start. */
const startsWith = (text: string) => new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`);

/** The `/v1/models` list of the server at `origin` (the page's own when empty). */
async function listed(page: Page, origin = ''): Promise<ModelInfo[]> {
  return ((await (await page.request.get(`${origin}/v1/models`)).json()) as { models: ModelInfo[] }).models;
}

/**
 * Run, held, sends nothing: neither a click (with `force`: Playwright waits on `aria-disabled` buttons) nor Ctrl+Enter
 * (`requests` holds every request the page made) starts a run or says anything, and it shows no tooltip saying it
 * would.
 */
async function expectSendsNothing(page: Page, requests: string[]) {
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'true');
  const sent = requests.length;
  const said = (await page.getByTestId('run-status').textContent()) ?? '';
  await runKey(page).click({ force: true });
  await page.keyboard.press('Control+Enter');
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(runKey(page)).toHaveAttribute('aria-busy', 'false');
  await expect(page.getByTestId('run-status')).toHaveText(said);
  expect(requests.slice(sent).filter(isSystemOne)).toEqual([]);
  // At the pill's faded strength a tooltip would also cover the banner.
  await runKey(page).hover({ force: true });
  expect(await runKey(page).evaluate((key) => getComputedStyle(key, '::after').display)).toBe('none');
}

/**
 * On a local server, the server row of `model`, which it has not pulled: Run is held and says how the model gets onto
 * this server; the snippets show `ardana pull <model>` and `ardana run` with the exact request, and no install line.
 */
async function expectPull(page: Page, model: string, request: Request, requests: string[]) {
  await expect(modelPicker(page)).toHaveValue(model);
  const reason = `This server has not pulled ${model}: pull it with the ardana CLI`;
  await expect(page.getByTestId('run-note')).toContainText(reason);
  await expect(runKey(page)).toHaveAccessibleDescription(startsWith(reason));
  await expect(page.getByTestId('cli-lede')).toHaveText(`This server runs ${model} once the ardana CLI pulls it.`);
  await expect(page.getByTestId('install')).toHaveCount(0);
  await expect(page.getByTestId('pull')).toHaveText(`ardana pull ${model}`);
  const command = await ardanaCommand(page);
  expect(command.line).toBe(`ardana run ${model} --request - <<'JSON'`);
  expect(command.body).toBe(body(model, request));
  await expectSendsNothing(page, requests);
}

/**
 * On a public server, the server row of `model`: Run is held and says it runs with the ardana CLI on the visitor's
 * machine, offering the browser default in this tab instead; the snippets show the install line and `ardana run` with
 * the exact request.
 */
async function expectInstall(page: Page, model: string, request: Request, requests: string[]) {
  await expect(modelPicker(page)).toHaveValue(model);
  const reason = `${model} runs with the ardana CLI on your machine`;
  await expect(page.getByTestId('run-note')).toContainText(reason);
  await expect(runKey(page)).toHaveAccessibleDescription(startsWith(reason));
  await expect(
    page.getByTestId('run-note').getByRole('button', { name: 'Run decider-0.8b in this tab instead' }),
  ).toBeVisible();
  await expect(page.getByTestId('cli-lede')).toHaveText(`${reason}.`);
  await expect(page.getByTestId('install')).toHaveText(INSTALL);
  await expect(page.getByTestId('pull')).toHaveCount(0);
  const command = await ardanaCommand(page);
  expect(command.line).toBe(`ardana run ${model} --request - <<'JSON'`);
  expect(command.body).toBe(body(model, request));
  await expectSendsNothing(page, requests);
}

test('run_command', async ({ page }, testInfo) => {
  const lacked = (await listed(page)).filter((m) => m.x_pulled === false);
  expect(lacked.map((m) => m.name)).toEqual(['decider-0.8b', 'decider-4b', 'qwen3.5-0.8b', 'smollm3-3b']);
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));

  // A share link naming a model this server has not pulled and no tab runs opens on its row.
  const ticket = fixture('ticket.json');
  await page.goto(`/${shareHash({ ...ticket, model: 'decider-4b' })}`);
  await expectPull(page, 'decider-4b', ticket, requests);
  const size = decimalSize(lacked.find((m) => m.name === 'decider-4b')!.x_size!);
  await expect(page.getByTestId('model-note')).toHaveText(
    `Not pulled on this server. Pull it with the ardana CLI where the server runs (${size}), then Run answers here.`,
  );
  await expect(page.getByTestId('pull-note')).toHaveText(`Downloads ${size}; then Run answers here.`);
  await expect(page.getByRole('heading', { level: 3, name: 'Sent', exact: true })).toBeVisible();
  await expect(page.getByText('Nothing sent yet.', { exact: true })).toBeVisible();
  // The library group says where these models run.
  await expect(modelPicker(page).locator('optgroup[label="Library · runs with the ardana CLI"] option')).toHaveText(
    lacked.map((m) => `${m.name} · ${decimalSize(m.x_size!)}`),
  );
  await screenshot(page, testInfo, 'run_command', 'held');

  // The banner's way to the commands opens Snippets on the pull.
  await page.getByRole('button', { name: 'Show the command' }).click();
  await expect(page.locator('#snippets > summary')).toBeFocused();
  await expect(page.getByTestId('pull')).toBeInViewport();
  await screenshot(page, testInfo, 'run_command', 'command');

  // Every other server row of a model this server has not pulled, decider-0.8b's among them: the same.
  for (const model of lacked.map((m) => m.name).filter((name) => name !== 'decider-4b')) {
    await pickModel(page, model);
    await expectPull(page, model, ticket, requests);
  }

  // Any spelling of a model the server has not pulled: a link naming decider-4b in other letters opens on its row, and
  // one naming another quant of it, a file the server would pull and the list does not size, holds Run with that pull,
  // with and without `?autorun=1`; nothing is sent.
  await page.goto(`/${shareHash({ ...ticket, model: 'Decider-4B' })}`);
  await expectPull(page, 'decider-4b', ticket, requests);
  const quant = 'decider-4b:q8_0';
  for (const autorun of ['', '?autorun=1']) {
    await page.goto(`/${autorun}${shareHash({ ...ticket, model: quant })}`);
    await expectPull(page, quant, ticket, requests);
    await expect(page.getByTestId('model-note')).toHaveText(
      'Not pulled on this server. Pull it with the ardana CLI where the server runs, then Run answers here.',
    );
    await expect(page.getByTestId('pull-note')).toHaveText('Then Run answers here.');
    expect(requests.filter(isSystemOne), autorun).toEqual([]);
  }
  await screenshot(page, testInfo, 'run_command', 'quant');

  // An "In browser" row: the ardana command without the install line, after `ardana pull` for a model this server has
  // not pulled (decider-0.8b), alone for one it has (decider-2b).
  await pickInBrowser(page, 'decider-0.8b');
  await expect(page.getByTestId('cli-lede')).toHaveText('The ardana CLI runs this request in a terminal.');
  await expect(page.getByTestId('install')).toHaveCount(0);
  await expect(page.getByTestId('pull')).toHaveText('ardana pull decider-0.8b');
  await expect(page.getByTestId('pull-note')).toHaveText(
    `Downloads ${decimalSize(lacked.find((m) => m.name === 'decider-0.8b')!.x_size!)}.`,
  );
  expect((await ardanaCommand(page)).line).toBe(`ardana run decider-0.8b --request - <<'JSON'`);
  await pickInBrowser(page, 'decider-2b');
  await expect(page.getByTestId('pull')).toHaveCount(0);
  await expect(page.getByTestId('install')).toHaveCount(0);
  expect((await ardanaCommand(page)).line).toBe(`ardana run decider-2b --request - <<'JSON'`);
  await screenshot(page, testInfo, 'run_command', 'in-browser');

  // The pulled model runs on the server again, with the API's snippets.
  await pickModel(page, 'decider-2b');
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  await expect(page.getByTestId('model-note')).toHaveCount(0);
  await expect(page.getByTestId('snippet')).toHaveAttribute('data-language', 'curl');
  const response = await run(page);
  expect(response.status()).toBe(200);
  expect(response.request().postDataJSON().model).toBe('decider-2b');
  expect(requests.filter(isSystemOne)).toHaveLength(1);
  await screenshot(page, testInfo, 'run_command');
});

// The model a local server has not pulled becomes a server row once `ardana pull` adds it, with no reload: the page
// lists the server's models again when the tab comes back. On an empty server of its own, so the other servers' lists
// stay as their cases expect; each project starts with qwen3.5-0.8b not pulled (`ardana rm` removes the entry the other
// project's pull added, never a file). Headless Chrome keeps every page visible and focused (a tab switch or a
// minimised window sends nothing), so the case sends the page the events a tab switch sends: `focus` at the window,
// `visibilitychange` at the document.
test('pull_while_open', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const base = process.env.ARDANA_PULL_URL!;
  const home = process.env.ARDANA_PULL_HOME!;
  expect(base && home, 'xtask passes the server of this case and its home').toBeTruthy();
  const model = 'qwen3.5-0.8b';
  // The ardana CLI under test, where this server runs: its home, offline over tmp/hf.
  const env = {
    ...process.env,
    PATH: `${path.dirname(process.env.ARDANA_BIN!)}:${process.env.PATH}`,
    ARDANA_HOME: home,
    HF_HUB_OFFLINE: '1',
  };
  const shell = (command: string) => execFileSync('bash', ['-c', command], { env, encoding: 'utf8', timeout: 300_000 });
  const pulled = async () => (await listed(page, base)).find((m) => m.name === model)?.x_pulled !== false;
  if (await pulled()) {
    shell(`ardana rm ${model}`);
  }
  await expect.poll(pulled).toBe(false);
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));

  await page.goto(`${base}/`);
  await loadPreset(page, 'Ticket routing');
  await pickModel(page, model);
  const ticket = fixture('ticket.json');
  await expectPull(page, model, ticket, requests);
  await screenshot(page, testInfo, 'pull_while_open', 'not-pulled');

  // The shown pull, run as shown where the server runs: the server serves the model at once; the page has not looked.
  const pull = (await page.getByTestId('pull').textContent())!;
  console.log(`pull_while_open ${testInfo.project.name}: ${pull}\n${shell(`${pull} 2>&1`)}`);
  await expect.poll(pulled).toBe(true);
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'true');

  // The tab comes back: the list says the server has pulled it, so its row is a server row and Run sends it.
  await page.evaluate(() => window.dispatchEvent(new FocusEvent('focus')));
  await expect(modelPicker(page).locator('optgroup[label="Pulled"] option')).toHaveText([model]);
  await expect(modelPicker(page)).toHaveValue(model);
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  await expect(page.getByTestId('run-note')).toBeHidden();
  await expect(page.getByTestId('model-note')).toHaveCount(0);
  await expect(page.getByTestId('snippet')).toHaveAttribute('data-language', 'curl');
  const response = await run(page);
  expect(response.status()).toBe(200);
  const answered = (await response.json()) as Json;
  expect(response.request().postDataJSON().model).toBe(model);
  await expect(page.getByTestId('answered-by')).toContainText(`Answered by ${model}`);
  await expect(page.getByTestId('answered-by')).not.toContainText('in this tab');
  await expectFiguresMatch(page, answered);
  await screenshot(page, testInfo, 'pull_while_open', 'answered');

  // Removed again, the page sees it when shown again: the row holds Run with the pull once more.
  shell(`ardana rm ${model}`);
  await expect.poll(pulled).toBe(false);
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange', { bubbles: true })));
  await expect(modelPicker(page).locator('optgroup[label="Pulled"]')).toHaveCount(0);
  await expectPull(page, model, ticket, requests);
  expect(requests.filter(isSystemOne)).toHaveLength(1);
  await screenshot(page, testInfo, 'pull_while_open');
});

test('public_playground', { tag: '@public' }, async ({ page, baseURL }, testInfo) => {
  test.setTimeout(900_000);
  const origin = new URL(baseURL!).origin;
  const models = await listed(page);
  expect(models.every((m) => m.x_pulled === false && !m.x_default)).toBe(true);
  const inBrowser = models.filter((m) => m.x_browser !== undefined);
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));

  // R3.5: the page opens on the browser default's row in the tab; every model is the CLI's on this server.
  await page.goto('/');
  const picker = modelPicker(page);
  await expect(picker).toHaveValue(browserRow('decider-0.8b'));
  await expect(picker.locator('optgroup[label="Pulled"]')).toHaveCount(0);
  await expect(picker.locator('optgroup[label="In browser · runs in this tab"] option')).toHaveText(
    inBrowser.map((m) => `${m.name} · ${decimalSize(m.x_browser!)}`),
  );
  await expect(picker.locator('optgroup[label="Library · runs with the ardana CLI"] option')).toHaveText(
    models.map((m) => `${m.name} · ${decimalSize(m.x_size!)}`),
  );
  await loadPreset(page, 'Ticket routing');
  const ticket = fixture('ticket.json');
  // R3.4: the tab's row shows ardana.ai's install line and the CLI's command for the same request.
  await expect(page.getByTestId('cli-lede')).toHaveText('The ardana CLI runs this request on your own machine.');
  await expect(page.getByTestId('install')).toHaveText(INSTALL);
  await expect(page.getByTestId('pull')).toHaveCount(0);
  const command = await ardanaCommand(page);
  expect(command.line).toBe(`ardana run decider-0.8b --request - <<'JSON'`);
  expect(command.body).toBe(body('decider-0.8b', ticket));
  expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
  await screenshot(page, testInfo, 'public_playground', 'opened');

  // R3.3: this server runs no model, so every server row runs with the ardana CLI on the visitor's machine, and Run
  // sends nothing.
  for (const model of models.map((m) => m.name)) {
    await pickModel(page, model);
    await expectInstall(page, model, ticket, requests);
    const size = decimalSize(models.find((m) => m.name === model)!.x_size!);
    await expect(page.getByTestId('model-note')).toHaveText(
      `Runs with the ardana CLI on your machine; its first run downloads ${size}.`,
    );
  }
  await screenshot(page, testInfo, 'public_playground', 'command');

  // The browser default runs in this tab instead: the banner's switch picks its "In browser" row and gives Run the
  // focus, described by what its first run downloads; the first Run answers there, and nothing goes to
  // /v1/systemone or to another origin (R3.5).
  await page.getByTestId('run-note').getByRole('button', { name: 'Run decider-0.8b in this tab instead' }).click();
  await expect(picker).toHaveValue(browserRow('decider-0.8b'));
  await expect(runKey(page)).toBeFocused();
  const firstRun = `Run downloads decider-0.8b into this tab once: ${decimalSize(
    inBrowser.find((m) => m.name === 'decider-0.8b')!.x_browser!,
  )}, kept by this browser.`;
  await expect(runKey(page)).toHaveAccessibleDescription(startsWith(firstRun));
  await screenshot(page, testInfo, 'public_playground', 'in-tab');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(page.getByTestId('fault')).toHaveCount(0);
  const answered = JSON.parse((await page.getByTestId('raw-response').textContent())!) as { model: string };
  expect(answered.model).toBe('decider-0.8b-v1');
  await expectFiguresMatch(page, answered as unknown as Json);
  expect(requests.filter(isSystemOne)).toEqual([]);
  for (const url of requests) {
    expect(new URL(url).origin, url).toBe(origin);
  }
  // An API client meets the server's own refusal.
  const refused = await page.request.post('/v1/systemone', { data: ticket });
  expect(refused.status()).toBe(403);
  expect(((await refused.json()) as { detail: { error_type: string } }).detail.error_type).toBe('permission_error');
  await screenshot(page, testInfo, 'public_playground');
});
