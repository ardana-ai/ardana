// The first run on an empty registry: `cargo xtask e2e playground` starts one more release `ardana serve` per viewport
// with an empty `ARDANA_HOME` and `HF_HUB_OFFLINE=1` over `tmp/hf` (`ARDANA_EMPTY_URL_<PROJECT>`). The picker starts
// on the library default, nothing on the page speaks of Jev, and RUN pulls decider-2b before it answers.
import { expect, test, type Page } from '@playwright/test';
import {
  decimalSize,
  expectFiguresMatch,
  fixture,
  isSystemOne,
  picker as modelPicker,
  runKey,
  screenshot,
  shareHash,
  type Json,
  type ModelInfo,
} from './helpers';

/** Every text a person can read on the page: the rendered text, the picker's option and group labels, the title. */
async function visibleText(page: Page): Promise<string> {
  return page.evaluate(() =>
    [
      document.title,
      document.body.innerText,
      ...[...document.querySelectorAll('option, optgroup')].map((el) => (el as HTMLOptionElement).label),
      ...[...document.querySelectorAll('[placeholder], [aria-label], [title]')].map((el) =>
        ['placeholder', 'aria-label', 'title'].map((a) => el.getAttribute(a) ?? '').join(' '),
      ),
    ].join('\n'),
  );
}

test('first_run_pull', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const base = process.env[`ARDANA_EMPTY_URL_${testInfo.project.name.toUpperCase()}`];
  expect(base, 'xtask passes the empty server of this viewport').toBeTruthy();

  // Nothing is pulled; decider-2b is the library default.
  const listed = (await (await page.request.get(`${base}/v1/models`)).json()) as { models: ModelInfo[] };
  expect(listed.models.every((m) => m.x_pulled === false)).toBe(true);
  const decider = listed.models.find((m) => m.x_default)!;
  expect(decider.name).toBe('decider-2b');

  // A share link as Jev writes it (`jev-latest`) opens on the local default model.
  const ticket = fixture('ticket.json');
  expect(ticket.model).toBe('jev-latest');
  await page.goto(`${base}/${shareHash(ticket)}`);
  const picker = modelPicker(page);
  await expect(picker).toHaveValue('decider-2b');
  await expect(picker.locator('optgroup[label="Pulled"]')).toHaveCount(0);
  await expect(picker.locator('optgroup[label="Library · pulls on first run"] option').first()).toHaveText(
    `decider-2b · ${decimalSize(decider.x_size!)}`,
  );
  expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
  await screenshot(page, testInfo, 'first_run_pull', 'empty');

  // RUN pulls first: record what the key and its note say while the request is in flight.
  await page.evaluate(() => {
    const seen: string[] = [];
    (window as unknown as { seen: string[] }).seen = seen;
    const record = () => {
      seen.push(`key:${document.querySelector('[data-testid="run-label"]')?.textContent ?? ''}`);
      seen.push(`note:${document.querySelector('[data-testid="run-note"]')?.textContent ?? ''}`);
    };
    new MutationObserver(record).observe(document.body, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  });
  const replied = page.waitForResponse((r) => isSystemOne(r.url()), { timeout: 0 });
  await runKey(page).click();
  const response = await replied;
  expect(response.status()).toBe(200);
  expect(response.request().postDataJSON().model).toBe('decider-2b');
  const seen = await page.evaluate(() => (window as unknown as { seen: string[] }).seen);
  expect(seen).toContain('key:Pulling');
  expect(seen).toContain(`note:Downloading decider-2b (${decimalSize(decider.x_size!)}) on first run`);

  // The answers are the API's, and decider-2b is pulled now.
  const json = (await response.json()) as { model: string; answers: Record<string, unknown> };
  expect(json.model).toBe('decider-2b-v11');
  await expect(page.getByTestId('fault')).toHaveCount(0);
  await expect(page.getByTestId('channel')).toHaveCount(Object.keys(ticket.questions).length);
  const fields = await expectFiguresMatch(page, json as unknown as Json);
  for (const id of Object.keys(ticket.questions)) {
    expect(fields.some((f) => f.startsWith(`/answers/${id}/`)), `${id} is displayed`).toBe(true);
  }
  await expect(page.getByTestId('run-note')).toBeEmpty();
  await expect(runKey(page)).toContainText('Run');
  await expect(picker.locator('optgroup[label="Pulled"] option')).toHaveText(['decider-2b']);
  await expect(picker).toHaveValue('decider-2b');
  const after = (await (await page.request.get(`${base}/v1/models`)).json()) as { models: ModelInfo[] };
  expect(after.models[0]).toMatchObject({ name: 'decider-2b', x_pulled: true, x_default: true });
  expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
  await screenshot(page, testInfo, 'first_run_pull', 'answered');
});
