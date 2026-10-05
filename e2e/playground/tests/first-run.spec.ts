// R3.5, the first run on an empty registry: `cargo xtask e2e playground` starts one more release `ardana serve` with
// an empty `ARDANA_HOME` and `HF_HUB_OFFLINE=1` over `tmp/hf` (`ARDANA_EMPTY_URL`). The default model (decider:2b)
// is not pulled, so the picker opens on the browser default's row in the tab (Q11), nothing on the page speaks of Jev,
// and the first Run answers in the tab: the registry stays empty for the next viewport.
import { expect, test } from '@playwright/test';
import {
  IN_MEMORY,
  browserRow,
  decimalSize,
  expectFiguresMatch,
  fixture,
  isSystemOne,
  picker as modelPicker,
  runKey,
  screenshot,
  shareHash,
  visibleText,
  type Json,
  type ModelInfo,
} from './helpers';

test('first_run', async ({ page }, testInfo) => {
  test.setTimeout(900_000);
  const base = process.env.ARDANA_EMPTY_URL;
  expect(base, 'xtask passes the empty server').toBeTruthy();

  // Nothing is pulled; decider:2b is the default model and decider:0.8b the browser default.
  const listed = (await (await page.request.get(`${base}/v1/models`)).json()) as { models: ModelInfo[] };
  expect(listed.models.every((m) => m.x_pulled === false)).toBe(true);
  expect(listed.models.find((m) => m.x_default)!.name).toBe('decider:2b');
  const browserDefault = listed.models.filter((m) => m.x_browser_default);
  expect(browserDefault.map((m) => m.name)).toEqual(['decider:0.8b']);
  const size = browserDefault[0].x_browser!;

  // A share link as Jev writes it (`jev-latest`) opens on the browser default's row in the tab.
  const ticket = fixture('ticket.json');
  expect(ticket.model).toBe('jev-latest');
  await page.goto(`${base}/${shareHash(ticket)}`);
  const picker = modelPicker(page);
  await expect(picker).toHaveValue(browserRow('decider:0.8b'));
  await expect(page.getByTestId('model-note')).toHaveText(
    `Runs in this tab. The first run downloads ${decimalSize(size)}, which this browser keeps. ${IN_MEMORY}; only ` +
      'a reload frees all of it.',
  );
  await expect(picker.locator('optgroup[label="Pulled"]')).toHaveCount(0);
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
  await screenshot(page, testInfo, 'first_run', 'opened');

  // The first Run answers in this tab: no request goes to /v1/systemone, and the server pulls no model.
  const requests: string[] = [];
  page.on('request', (r) => requests.push(r.url()));
  await runKey(page).click();
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await expect(page.getByTestId('fault')).toHaveCount(0);
  await expect(page.getByTestId('raw-status')).toHaveText('HTTP 200');
  const json = JSON.parse((await page.getByTestId('raw-response').textContent())!) as {
    model: string;
    answers: Record<string, unknown>;
  };
  expect(json.model).toBe('decider-0.8b-v1');
  expect(Object.keys(json.answers)).toEqual(Object.keys(ticket.questions));
  const fields = await expectFiguresMatch(page, json as unknown as Json);
  for (const id of Object.keys(ticket.questions)) {
    expect(fields.some((f) => f.startsWith(`/answers/${id}/`)), `${id} is displayed`).toBe(true);
  }
  expect(requests.filter(isSystemOne)).toEqual([]);
  const after = (await (await page.request.get(`${base}/v1/models`)).json()) as { models: ModelInfo[] };
  expect(after.models.every((m) => m.x_pulled === false)).toBe(true);
  await expect(picker).toHaveValue(browserRow('decider:0.8b'));
  expect((await visibleText(page)).toLowerCase()).not.toContain('jev');
  await screenshot(page, testInfo, 'first_run', 'answered');
});
