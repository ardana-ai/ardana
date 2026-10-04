// W3, the models a server cannot run, as the user amended Q2, Q4 and R3.3. On a local server, which ardana runs
// already, a model it has not pulled shows `ardana pull <name>` and `ardana run <name> --request -` with the exact
// request, never ardana.ai's install line, and once `ardana pull` has added it the page runs it on the server. Run
// sends nothing for a model the server would have to pull, autorun included. `run_command` runs on the release
// `ardana serve` holding decider-2b, `pull_while_open` on an empty server of its own (the playground suite).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  ardanaCommand,
  body,
  decimalSize,
  expectFiguresMatch,
  expectSendsNothing,
  fixture,
  isSystemOne,
  loadPreset,
  pickInBrowser,
  pickModel,
  picker as modelPicker,
  root,
  run,
  runKey,
  screenshot,
  shareHash,
  startsWith,
  type Json,
  type ModelInfo,
  type Request,
} from './helpers';

/** The `/v1/models` list of the server at `origin` (the page's own when empty). */
async function listed(page: Page, origin = ''): Promise<ModelInfo[]> {
  return ((await (await page.request.get(`${origin}/v1/models`)).json()) as { models: ModelInfo[] }).models;
}

/** The names of the models the library snapshot (C5) lists, in its order: the library the servers under test read. */
const snapshotNames = (): string[] =>
  (
    JSON.parse(fs.readFileSync(path.join(root, 'crates/ardana-registry/tests/data/models.json'), 'utf8')) as {
      models: { name: string }[];
    }
  ).models.map((m) => m.name);

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

test('run_command', async ({ page }, testInfo) => {
  // This server has pulled decider-2b alone: every other library model is one it lacks.
  const lacked = (await listed(page)).filter((m) => m.x_pulled === false);
  expect(lacked.map((m) => m.name)).toEqual(snapshotNames().filter((name) => name !== 'decider-2b'));
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
