// W7 cases, Jev playground parity: the builder, state modes, the docs share links, the share round trip, the presets
// and the snippets, against the release `ardana` serving decider-2b (see playwright.config.ts).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import LZString from 'lz-string';
import {
  channel,
  editorTexts,
  expectFiguresMatch,
  loadPreset,
  pickModel,
  picker,
  pointer,
  readRequest,
  restorePrevious,
  root,
  run,
  runKey,
  screenshot,
  shareHash,
  type Json,
  type Request,
} from './helpers';

const stateBox = (page: Page) => page.getByRole('textbox', { name: 'State' });
const questionsBox = (page: Page) => page.getByRole('textbox', { name: 'Questions JSON' });
const questionsJson = async (page: Page) => JSON.parse(await questionsBox(page).inputValue()) as Record<string, Json>;

/** The model `ardana serve` has pulled: decider-2b. */
/** The server's default model, which the picker starts on. */
async function servedModel(page: Page): Promise<string> {
  const list = (await (await page.request.get('/v1/models')).json()) as { models: { name: string; x_default?: boolean }[] };
  return list.models.find((m) => m.x_default)!.name;
}

/** A listed model other than the default one (a library model; picking it runs nothing). */
async function otherModel(page: Page): Promise<string> {
  const list = (await (await page.request.get('/v1/models')).json()) as { models: { name: string; x_default?: boolean }[] };
  return list.models.find((m) => !m.x_default)!.name;
}

/** Every displayed figure's field and raw value. */
async function figures(page: Page): Promise<Map<string, string>> {
  const pairs = await page
    .locator('[data-field]')
    .evaluateAll((els) => els.map((el) => [el.getAttribute('data-field')!, el.getAttribute('data-value')!] as const));
  return new Map(pairs);
}

test('builder_sync', async ({ page }, testInfo) => {
  await page.goto('/');

  // A new noul question opens its builder; every builder edit rewrites the questions JSON at once.
  await page.getByRole('button', { name: 'Add question' }).click();
  const q1 = channel(page, 'q1');
  await expect(q1).toHaveCount(1);
  await expect(questionsBox(page)).toHaveValue(JSON.stringify({ q1: { type: 'noul', instructions: '' } }, null, 2));
  await q1.getByRole('textbox', { name: 'Instructions' }).fill('Does the customer ask for a refund?');
  await q1.getByRole('textbox', { name: 'Yes means (optional)' }).fill('they want money back');
  await q1.getByRole('textbox', { name: 'No means (optional)' }).fill('anything else');
  expect(await questionsJson(page)).toEqual({
    q1: {
      type: 'noul',
      instructions: 'Does the customer ask for a refund?',
      criteria: { true: 'they want money back', false: 'anything else' },
    },
  });
  await q1.getByRole('textbox', { name: 'Yes means (optional)' }).fill('');
  expect((await questionsJson(page)).q1).toEqual({
    type: 'noul',
    instructions: 'Does the customer ask for a refund?',
    criteria: { false: 'anything else' },
  });

  // Renaming keeps the question's place; a taken or empty id is refused inline.
  const id = q1.getByRole('textbox', { name: 'Question id' });
  await id.fill('refund');
  await id.press('Tab');
  const refund = channel(page, 'refund');
  await expect(refund.getByRole('textbox', { name: 'Question id' })).toHaveValue('refund');
  // The block is drawn again under its new id, and focus follows the Tab to its Instructions field.
  await expect(refund.getByRole('textbox', { name: 'Instructions' })).toBeFocused();
  expect(Object.keys(await questionsJson(page))).toEqual(['refund']);

  // The type toggle is a radio group: arrow keys switch noul to choice, which starts with two named options.
  await refund.getByRole('radio', { name: 'noul', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(refund.getByRole('radio', { name: 'choice', exact: true })).toBeChecked();
  expect((await questionsJson(page)).refund).toEqual({
    type: 'choice',
    instructions: 'Does the customer ask for a refund?',
    criteria: { option_1: null, option_2: null },
  });
  await expect(refund.getByRole('button', { name: 'Remove option 1' })).toBeDisabled();
  await refund.getByRole('textbox', { name: 'Option 1 name' }).fill('refund');
  await refund.getByRole('textbox', { name: 'Option 2 name' }).fill('replacement');
  await refund.getByRole('button', { name: 'Add option' }).click();
  const third = refund.getByRole('textbox', { name: 'Option 3 name' });
  await third.fill('refund');
  await expect(refund.getByText('another option is already named "refund"')).toBeVisible();
  await expect(third).toHaveAttribute('aria-invalid', 'true');
  await expect(third).toHaveAccessibleDescription('another option is already named "refund"');
  await expect(page.getByTestId('notice')).toHaveText('Option 3 name: another option is already named "refund"');
  expect(Object.keys((await questionsJson(page)).refund.criteria as object)).toEqual(['refund', 'replacement', 'option_3']);
  await third.fill('neither');
  await expect(third).toHaveAttribute('aria-invalid', 'false');
  await refund.getByRole('textbox', { name: 'Option 2 description' }).fill('they want the item sent again');
  expect((await questionsJson(page)).refund.criteria).toEqual({
    refund: null,
    replacement: 'they want the item sent again',
    neither: null,
  });
  // A type round trip gives each type its own criteria back.
  const choiceCriteria = (await questionsJson(page)).refund.criteria;
  await refund.getByRole('radio', { name: 'noul', exact: true }).click();
  expect((await questionsJson(page)).refund.criteria).toEqual({ false: 'anything else' });
  await refund.getByRole('radio', { name: 'score', exact: true }).click();
  await refund.getByRole('radio', { name: 'choice', exact: true }).click();
  expect((await questionsJson(page)).refund.criteria).toEqual(choiceCriteria);
  await refund.getByRole('button', { name: 'Remove option 3' }).click();
  await expect(refund.getByRole('button', { name: 'Remove option 2' })).toBeDisabled();
  // The removed row was the last: focus moves to Add option.
  await expect(refund.getByRole('button', { name: 'Add option' })).toBeFocused();

  // A second question, switched to score with the mouse: 2..10 ordered levels.
  await page.getByRole('button', { name: 'Add question' }).click();
  const q2 = channel(page, 'q2');
  await q2.getByRole('radio', { name: 'score', exact: true }).click();
  await expect(refund.getByRole('textbox', { name: 'Question id' })).toHaveCount(0);
  await q2.getByRole('textbox', { name: 'Instructions' }).fill('How upset is the customer?');
  await q2.getByRole('textbox', { name: 'Level 0 description' }).fill('calm');
  await q2.getByRole('textbox', { name: 'Level 1 description' }).fill('annoyed');
  const addLevel = q2.getByRole('button', { name: 'Add level' });
  for (let level = 2; level < 10; level++) {
    await addLevel.click();
  }
  await expect(addLevel).toBeDisabled();
  await q2.getByRole('button', { name: 'Remove level 5' }).click();
  // Focus moves to the Remove key of the level that took its place.
  await expect(q2.getByRole('button', { name: 'Remove level 5' })).toBeFocused();
  for (let level = 8; level >= 2; level--) {
    await q2.getByRole('button', { name: `Remove level ${level}` }).click();
  }
  await expect(q2.getByRole('button', { name: 'Remove level 0' })).toBeDisabled();
  await addLevel.click();
  await expect(q2.getByRole('button', { name: 'Remove level 0' })).toBeEnabled();
  await q2.getByRole('textbox', { name: 'Level 2 description' }).fill('furious');
  expect((await questionsJson(page)).q2).toEqual({
    type: 'score',
    instructions: 'How upset is the customer?',
    criteria: ['calm', 'annoyed', 'furious'],
  });
  await screenshot(page, testInfo, 'builder_sync', 'editing');

  // JSON edits update the builder: a 254-option choice takes one more option, then no more.
  const built = await questionsJson(page);
  const many = Object.fromEntries(Array.from({ length: 254 }, (_, i) => [`o${i}`, null]));
  await questionsBox(page).fill(
    JSON.stringify({ ...built, many: { type: 'choice', instructions: 'Which one?', criteria: many } }, null, 2),
  );
  await expect(page.getByTestId('channel')).toHaveCount(3);
  await channel(page, 'many').getByRole('button', { name: 'Edit question many' }).click();
  await expect(channel(page, 'many').getByRole('textbox', { name: 'Option 254 name' })).toHaveValue('o253');
  await channel(page, 'many').getByRole('button', { name: 'Add option' }).click();
  await expect(channel(page, 'many').getByRole('textbox', { name: 'Option 255 name' })).toHaveValue('option_255');
  await expect(channel(page, 'many').getByRole('button', { name: 'Add option' })).toBeDisabled();
  await questionsBox(page).fill(JSON.stringify(built, null, 2));
  await expect(page.getByTestId('channel')).toHaveCount(2);
  await refund.getByRole('button', { name: 'Edit question refund' }).click();
  await expect(refund.getByRole('textbox', { name: 'Option 2 name' })).toHaveValue('replacement');
  await expect(refund.getByRole('textbox', { name: 'Option 2 description' })).toHaveValue('they want the item sent again');

  // Invalid JSON: an inline error, RUN held, and the builder keeps the last map that parsed.
  await questionsBox(page).fill('{"refund": {"type": "noul",');
  const parseError = 'Questions JSON ends early at line 1, column 27';
  await expect(page.locator('#questions-error')).toHaveText(parseError);
  await expect(questionsBox(page)).toHaveAttribute('aria-invalid', 'true');
  await expect(questionsBox(page)).toHaveAccessibleDescription(parseError);
  await expect(page.getByTestId('notice')).toHaveText(parseError);
  // RUN is held, with its reason beside it and in its description.
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'true');
  await expect(runKey(page)).toHaveAccessibleDescription('Questions JSON has an error');
  await expect(page.locator('#run-note')).toBeVisible();
  await expect(page.getByTestId('channel')).toHaveCount(2);
  await expect(refund.getByRole('radio', { name: 'choice', exact: true })).toBeChecked();
  await expect(refund.getByRole('textbox', { name: 'Option 1 name' })).toHaveValue('refund');
  // The next builder edit writes the kept map back.
  await refund.getByRole('textbox', { name: 'Option 1 name' }).fill('money_back');
  await expect(page.locator('#questions-error')).toHaveText('');
  await expect(page.getByTestId('notice')).toHaveText('Questions JSON is valid again');
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  expect(Object.keys((await questionsJson(page)).refund.criteria as object)).toEqual(['money_back', 'replacement']);

  // Remove question moves focus to Add question, and Restore previous brings the question back until the next edit.
  await page.getByRole('button', { name: 'Add question' }).click();
  const beforeRemoval = await questionsBox(page).inputValue();
  await channel(page, 'q3').getByRole('button', { name: 'Remove question' }).click();
  await expect(page.getByTestId('channel')).toHaveCount(2);
  await expect(page.getByRole('button', { name: 'Add question' })).toBeFocused();
  await restorePrevious(page);
  await expect(questionsBox(page)).toHaveValue(beforeRemoval);
  await expect(stateBox(page)).toBeFocused();
  await expect(page.getByRole('button', { name: 'Restore previous' })).toHaveCount(0);
  await channel(page, 'q3').getByRole('button', { name: 'Edit question q3' }).click();

  // The built request runs: noul, choice and score answered, every figure as the API returned it.
  await channel(page, 'q3').getByRole('textbox', { name: 'Instructions' }).fill('Is the order number given?');
  await stateBox(page).fill('Order A-104 arrived broken and I am really upset. Please send it again.');
  const response = await run(page);
  expect(response.status()).toBe(200);
  const json = (await response.json()) as { answers: Record<string, { type: string }> };
  expect(Object.entries(json.answers).map(([id, a]) => [id, a.type])).toEqual([
    ['refund', 'choice'],
    ['q2', 'score'],
    ['q3', 'noul'],
  ]);
  await expectFiguresMatch(page, json as unknown as Json);
  await screenshot(page, testInfo, 'builder_sync');
});

test('state_modes', async ({ page }, testInfo) => {
  const question: Request = {
    state: '',
    questions: { customer: { type: 'noul', instructions: 'Does the state name a customer?' } },
  };
  await page.goto(`/${shareHash(question)}`);
  const cases: [string, Json, 'JSON' | 'text'][] = [
    ['{"customer": "Ana", "items": [1, 2]}', { customer: 'Ana', items: [1, 2] }, 'JSON'],
    ['  [{"from": "Ana", "text": "refund please"}]\n', [{ from: 'Ana', text: 'refund please' }], 'JSON'],
    ['[]', [], 'JSON'],
    ['Ana asks for a refund.', 'Ana asks for a refund.', 'text'],
    ['42', '42', 'text'],
    ['"Ana"', '"Ana"', 'text'],
    ['null', 'null', 'text'],
    ['{"customer": "Ana",', '{"customer": "Ana",', 'text'],
  ];
  for (const [text, sent, mode] of cases) {
    await stateBox(page).fill(text);
    await expect(page.getByTestId('state-mode')).toHaveText(`Sent as ${mode}`);
    const response = await run(page);
    expect(response.status(), text).toBe(200);
    expect(response.request().postDataJSON().state, text).toEqual(sent);
    await expect(page.getByTestId('raw-request')).toHaveJSProperty('textContent', response.request().postData()!);
  }
  await screenshot(page, testInfo, 'state_modes');
});

type DocsLink = { page: string; url: string };
const docs = JSON.parse(fs.readFileSync(path.join(root, 'e2e/playground/fixtures/jev-share-links.json'), 'utf8')) as {
  links: DocsLink[];
};

test.describe('jev_share_links', () => {
  test('all 17 docs links are vendored', () => {
    expect(docs.links).toHaveLength(17);
    expect(new Set(docs.links.map((l) => l.url)).size).toBe(17);
    for (const link of docs.links) {
      expect(link.page.startsWith('https://docs.typesafe.ai/'), link.page).toBe(true);
      expect(link.url.startsWith('https://console.typesafe.ai/playground#share/'), link.url).toBe(true);
    }
  });

  for (const link of docs.links) {
    const slug = link.page.split('/').pop()!;
    test(slug, async ({ page }, testInfo) => {
      // Some docs requests hold dozens of questions over a 12k-token state; each row decodes the state again.
      test.setTimeout(900_000);
      const encoded = link.url.split('#share/')[1];
      const decoded = LZString.decompressFromEncodedURIComponent(encoded);
      expect(decoded, 'lz-string decodes the link').toBeTruthy();
      const payload = JSON.parse(decoded!) as {
        apiVersion: string;
        documentText: string;
        promptsText: string;
        selectedModels?: string[];
      };
      expect(payload.apiVersion).toBe('v1');
      const ids = Object.keys(JSON.parse(payload.promptsText));

      await page.goto(`/#share/${encoded}`);
      await expect(stateBox(page)).toHaveValue(payload.documentText);
      await expect(questionsBox(page)).toHaveValue(payload.promptsText);
      await expect(page.getByTestId('share-error')).toHaveCount(0);
      await expect(page.getByTestId('channel')).toHaveCount(ids.length);
      // A link without a model, or with a `jev-*` alias, opens on the server's default model.
      const served = await servedModel(page);
      const linked = payload.selectedModels?.[0];
      const model = linked === undefined || linked.startsWith('jev-') ? served : linked;
      await expect(picker(page)).toHaveValue(model);

      if (model !== served) {
        // A model this server does not have is refused with the names it has (Q7); then run on decider-2b.
        const refused = await run(page);
        expect(refused.status()).toBe(404);
        await expect(page.getByTestId('fault-message')).toContainText(`no model named "${model}"`);
        await pickModel(page, served);
      }
      const response = await run(page);
      expect(response.status()).toBe(200);
      const json = (await response.json()) as { answers: Record<string, { type: string }> };
      expect(Object.keys(json.answers)).toEqual(ids);
      const fields = await expectFiguresMatch(page, json as unknown as Json);
      for (const id of ids) {
        const shown = fields.filter((f) => f.startsWith(`${pointer('answers', id)}/`));
        expect(shown.length, `${id} shows its answer`).toBeGreaterThan(0);
        await expect(channel(page, id).locator('.readout.idle')).toHaveCount(0);
      }
      await screenshot(page, testInfo, 'jev_share_links', slug);
    });
  }
});

test('share_roundtrip', async ({ page, browser, baseURL }, testInfo) => {
  const origin = new URL(baseURL!).origin;
  await page.goto('/');
  await loadPreset(page, 'Support-chat audit');
  const transcript = await stateBox(page).inputValue();
  await stateBox(page).fill(transcript.replace('OK, thanks.', 'OK, merci ☕'));
  const model = await servedModel(page);
  await pickModel(page, model);
  const state = await stateBox(page).inputValue();
  const questions = await questionsBox(page).inputValue();

  await page.getByRole('button', { name: 'Share link' }).click();
  const link = await page.getByRole('textbox', { name: 'Link to these inputs' }).inputValue();
  expect(link.startsWith(`${origin}/#share/`), link).toBe(true);
  const decoded = LZString.decompressFromEncodedURIComponent(link.split('#share/')[1]);
  expect(decoded, 'lz-string 1.5.0 decodes the link').toBeTruthy();
  const payload = JSON.parse(decoded!);
  expect(Object.keys(payload)).toEqual(['apiVersion', 'documentText', 'promptsText', 'selectedModels']);
  expect(payload).toEqual({ apiVersion: 'v1', documentText: state, promptsText: questions, selectedModels: [model] });
  await screenshot(page, testInfo, 'share_roundtrip', 'link');

  const context = await browser.newContext({ viewport: testInfo.project.use.viewport });
  const fresh = await context.newPage();
  await fresh.goto(link);
  await expect(stateBox(fresh)).toHaveValue(state);
  await expect(questionsBox(fresh)).toHaveValue(questions);
  await expect(picker(fresh)).toHaveValue(model);
  await expect(fresh.getByTestId('channel')).toHaveCount(Object.keys(JSON.parse(questions)).length);
  await screenshot(fresh, testInfo, 'share_roundtrip');
  await context.close();
});

test('stale_and_restore', async ({ page }, testInfo) => {
  await page.goto('/');
  await loadPreset(page, 'Ticket routing');
  const ticketState = await stateBox(page).inputValue();
  expect((await run(page)).status()).toBe(200);
  const changed = page.getByTestId('changed');
  await expect(changed).not.toHaveClass(/\bon\b/);
  await expect(page.getByTestId('stale')).toHaveCount(0);
  if (testInfo.project.name === 'mobile') {
    // On a phone the answers are brought up to the top after RUN.
    await expect(page.getByTestId('channel').first()).toBeInViewport();
  }
  const department = channel(page, 'department');
  const refund = channel(page, 'refund');
  await expect(department.locator('[data-testid="ladder"].lit')).toHaveCount(1);
  await expect(department).not.toHaveClass(/\bstale\b/);
  const figuresBefore = await page.locator('[data-field]').evaluateAll((els) => els.map((e) => e.getAttribute('data-value')));

  // One question edited: only its channel is from the last run; the figures stay exactly as returned.
  await refund.getByRole('button', { name: 'Edit question refund' }).click();
  await refund.getByRole('textbox', { name: 'Instructions' }).fill('Does the customer want money back?');
  await expect(changed).toHaveClass(/\bon\b/);
  await expect(refund.getByTestId('stale')).toHaveText('From last run · inputs changed');
  await expect(department.getByTestId('stale')).toHaveCount(0);
  await expect(page.getByTestId('answered-by').getByTestId('stale')).toBeVisible();

  // The state edited: every question is from the last run and marked stale, figures unchanged.
  await stateBox(page).fill(`${ticketState} Also, the app crashes.`);
  await expect(department.getByTestId('stale')).toBeVisible();
  await expect(department).toHaveClass(/\bstale\b/);
  expect(await page.locator('[data-field]').evaluateAll((els) => els.map((e) => e.getAttribute('data-value')))).toEqual(
    figuresBefore,
  );
  await screenshot(page, testInfo, 'stale_and_restore', 'stale');

  // Running again clears it.
  expect((await run(page)).status()).toBe(200);
  await expect(changed).not.toHaveClass(/\bon\b/);
  await expect(page.getByTestId('stale')).toHaveCount(0);

  // The model changed: stale again.
  await pickModel(page, await otherModel(page));
  await expect(changed).toHaveClass(/\bon\b/);

  // A preset over edited work can be undone once; the next edit retires the offer.
  const edited = await stateBox(page).inputValue();
  const questions = await questionsBox(page).inputValue();
  await loadPreset(page, 'Resume screening');
  await expect(stateBox(page)).not.toHaveValue(edited);
  await restorePrevious(page);
  await expect(stateBox(page)).toHaveValue(edited);
  await expect(questionsBox(page)).toHaveValue(questions);
  await loadPreset(page, 'Support-chat audit');
  await stateBox(page).fill('A new state.');
  await expect(page.getByRole('button', { name: 'Restore previous' })).toHaveCount(0);
  await screenshot(page, testInfo, 'stale_and_restore');
});

test('presets', async ({ page }, testInfo) => {
  const presets: [string, string, Record<string, string>][] = [
    ['Ticket routing', 'tests/fixtures/requests/ticket.json', { department: 'choice', refund: 'noul' }],
    ['Resume screening', 'crates/ardana-playground/presets/resume-screening.json', { fit: 'score', five_years: 'noul' }],
    [
      'Support-chat audit',
      'crates/ardana-playground/presets/support-chat-audit.json',
      { apologized: 'noul', status: 'choice' },
    ],
  ];
  await page.goto('/');
  for (const [name, file, types] of presets) {
    const request = readRequest(file);
    await loadPreset(page, name);
    const { documentText, promptsText } = editorTexts(request);
    await expect(stateBox(page)).toHaveValue(documentText);
    await expect(questionsBox(page)).toHaveValue(promptsText);
    const response = await run(page);
    expect(response.status(), name).toBe(200);
    const json = (await response.json()) as { answers: Record<string, Record<string, Json>> };
    expect(Object.fromEntries(Object.entries(json.answers).map(([id, a]) => [id, a.type]))).toEqual(types);
    const fields = await expectFiguresMatch(page, json as unknown as Json);
    for (const [id, answer] of Object.entries(json.answers)) {
      const expected =
        answer.type === 'noul'
          ? [pointer('answers', id, 'noul')]
          : Object.keys(answer.probabilities as object).map((o) => pointer('answers', id, 'probabilities', o));
      for (const field of expected) {
        expect(fields, `${name}: ${field} is displayed`).toContain(field);
      }
    }
    if (name === 'Resume screening') {
      expect(Object.keys(json.answers.fit.probabilities as object)).toEqual(['0', '1', '2', '3']);
      expect(fields).toContain('/answers/fit/score');
    }
    if (name === 'Support-chat audit') {
      expect(Object.keys(json.answers.status.probabilities as object)).toEqual(['resolved', 'escalated', 'open']);
    }
    await screenshot(page, testInfo, 'presets', name.toLowerCase().replace(/[^a-z]+/g, '-'));
  }
});

test('snippets', async ({ page, baseURL }, testInfo) => {
  test.setTimeout(600_000);
  const origin = new URL(baseURL!).origin;
  const dir = path.join(root, 'tmp/playwright/snippets', testInfo.project.name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  // The TypeScript snippet imports @typesafe-ai/sdk, installed with the suite.
  fs.symlinkSync(path.join(root, 'e2e/playground/node_modules'), path.join(dir, 'node_modules'));
  const env = { ...process.env, TYPESAFE_BASE_URL: origin, TYPESAFE_API_KEY: 'ardana-e2e-unused' };

  await page.goto('/');
  await loadPreset(page, 'Ticket routing');
  const response = await run(page);
  expect(response.status()).toBe(200);
  const shown = await figures(page);
  expect(shown.size).toBeGreaterThan(0);
  // The SDKs model the official answer fields and drop Ardana's `x_` extras; the curl check covers those.
  const answersShown = [...shown].filter(
    ([field]) => field.startsWith('/answers/') && !field.split('/').some((part) => part.startsWith('x_')),
  );
  const snippet = page.getByTestId('snippet');
  const language = (name: string) => page.getByRole('radio', { name, exact: true });

  // curl: its body is the request the page sent, and it gets the very response the page shows.
  await expect(language('curl')).toBeChecked();
  const curl = (await snippet.textContent())!;
  expect(curl.startsWith(`curl -sS ${origin}/v1/systemone `)).toBe(true);
  expect(curl.split("<<'JSON'\n")[1].split('\nJSON\n')[0]).toBe(response.request().postData());
  fs.writeFileSync(path.join(dir, 'decide.sh'), curl);
  const curlOut = JSON.parse(execFileSync('bash', ['decide.sh'], { cwd: dir, env, encoding: 'utf8', timeout: 300_000 }));
  expect(curlOut).toEqual(await response.json());

  // The SDK snippets: each figure on screen equals the answer the snippet printed.
  const sdks: [string, string, string[]][] = [
    ['Python', 'decide.py', [path.join(root, 'tmp/py/sdk/bin/python'), 'decide.py']],
    ['TypeScript', 'decide.mts', [process.execPath, 'decide.mts']],
  ];
  for (const [name, file, [program, ...args]] of sdks) {
    await language(name).check();
    await expect(snippet).toHaveAttribute('aria-label', `${name} snippet`);
    const text = (await snippet.textContent())!;
    if (name === 'Python') {
      expect(text).toContain(`TYPESAFE_BASE_URL=${origin} `);
    } else {
      expect(text).toContain(`baseURL: "${origin}"`);
    }
    fs.writeFileSync(path.join(dir, file), text);
    const out = JSON.parse(execFileSync(program, args, { cwd: dir, env, encoding: 'utf8', timeout: 300_000 }));
    for (const [field, value] of answersShown) {
      const got = field.split('/').slice(1).reduce<Json | undefined>(
        (node, key) => (node as Record<string, Json> | undefined)?.[key.replace(/~1/g, '/').replace(/~0/g, '~')],
        out,
      );
      expect(got, `${name}: ${field}`).not.toBeUndefined();
      expect(typeof got === 'number' ? got : String(got), `${name}: ${field}`).toBe(
        typeof got === 'number' ? Number(value) : value,
      );
    }
  }

  // The snippets follow the editors: a new state and model are in the next snippet.
  await stateBox(page).fill('I was charged twice for order B-7.');
  await pickModel(page, await otherModel(page));
  await language('curl').check();
  const body = JSON.parse((await snippet.textContent())!.split("<<'JSON'\n")[1].split('\nJSON\n')[0]);
  expect(body.state).toBe('I was charged twice for order B-7.');
  expect(body.model).toBe(await otherModel(page));
  await language('TypeScript').check();
  await expect(snippet).toContainText('state: "I was charged twice for order B-7."');
  await screenshot(page, testInfo, 'snippets');
});
