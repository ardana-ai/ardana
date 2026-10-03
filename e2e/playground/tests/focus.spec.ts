// Focus, history and announcements after what the page does on its own, against the release `ardana` serving
// decider-2b (see playwright.config.ts): the skip link (`skip_link`), the Share popover (`share_popover`), the live
// regions (`announcements`), a run's answers on a phone (`run_focus`), the ids ARIA names (`aria_relationships`), the
// fact rows and the names Chrome gives controls with tooltips (`term_stops`), and the focus after Add and Stop
// (`page_focus`).
import { type CDPSession, expect, type Locator, type Page, test } from '@playwright/test';
import {
  browserRow,
  channel,
  editorTexts,
  fixture,
  inFlight,
  isSystemOne,
  loadPreset,
  pickInBrowser,
  pickModel,
  picker,
  readRequest,
  restorePrevious,
  run,
  runKey,
  screenshot,
  shareHash,
  stopInFlight,
  throttle,
  type Json,
  type Request,
} from './helpers';

/** The ticket with a question the API refuses (a choice of one option): a 422 that names `department`. */
function refused(): Request {
  const invalid = structuredClone(fixture('ticket.json'));
  (invalid.questions.department as Record<string, Json>).criteria = ['billing'];
  return invalid;
}

/** The name Chrome's accessibility tree gives the focused element. */
async function focusedName(cdp: CDPSession): Promise<string | undefined> {
  const { nodes } = (await cdp.send('Accessibility.getFullAXTree')) as {
    nodes: { name?: { value?: string }; properties?: { name: string; value: { value?: unknown } }[] }[];
  };
  // The document has the focus too: the element holding it is the last focused node in the tree's order.
  const focused = nodes.filter((node) =>
    node.properties?.some((property) => property.name === 'focused' && property.value.value === true),
  );
  return focused.at(-1)?.name?.value;
}

/** Whether `target` shows whole below what stays of the top bar and the banner. */
function underTheBars(target: Locator) {
  return target.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const cover = Math.max(
      0,
      document.querySelector('.topbar')!.getBoundingClientRect().bottom,
      document.querySelector('#run-note')!.getBoundingClientRect().bottom,
    );
    return box.top >= cover - 0.5 && box.bottom <= innerHeight + 0.5;
  });
}

// "Skip to the page" focuses and scrolls to the page without a fragment: the URL and the history stay as they were, so
// Back never loads the share link again over the edits. A share link loaded over edited editors keeps them, with the
// row they had, one Restore previous away. Under the open drawer the page is inert, and so is the skip link.
test('skip_link', async ({ page }, testInfo) => {
  const ticket = fixture('ticket.json');
  const state = page.getByRole('textbox', { name: 'State' });
  await page.goto('/');
  await expect(picker(page)).toHaveValue('decider-2b');
  // A share link opened on the page, a history entry of its own, then edited.
  await page.evaluate((hash) => {
    location.hash = hash;
  }, shareHash({ ...ticket, model: 'decider-2b' }));
  await expect(state).toHaveValue(ticket.state as string);
  const edited = `${ticket.state} Also, the app crashes.`;
  await state.fill(edited);
  const before = await page.evaluate(() => ({ url: location.href, entries: history.length }));
  await page.locator('.skip-link').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#content')).toBeFocused();
  expect(await page.evaluate(() => ({ url: location.href, entries: history.length }))).toEqual(before);
  await expect(page.getByRole('heading', { level: 1 })).toBeInViewport();
  // Back goes to the page before the share link, and the edits stay.
  await page.goBack();
  await expect(page).toHaveURL((url) => url.hash === '');
  await expect(state).toHaveValue(edited);

  // Another share link over the edits, with another row picked: Restore previous brings both back.
  await pickInBrowser(page, 'decider-0.8b');
  const resume = readRequest('crates/ardana-playground/presets/resume-screening.json');
  await page.evaluate((hash) => {
    location.hash = hash;
  }, shareHash({ ...resume, model: 'decider-2b' }));
  await expect(state).toHaveValue(editorTexts(resume).documentText);
  await expect(picker(page)).toHaveValue('decider-2b');
  await screenshot(page, testInfo, 'skip_link', 'replaced');
  await restorePrevious(page);
  await expect(state).toHaveValue(edited);
  await expect(page.getByRole('textbox', { name: 'Questions JSON' })).toHaveValue(editorTexts(ticket).promptsText);
  await expect(picker(page)).toHaveValue(browserRow('decider-0.8b'));
  // A link over the edits, then the same link again: the editors hold it already, and the edits stay on offer.
  const again = async () =>
    page.evaluate((hash) => {
      location.hash = '';
      location.hash = hash;
    }, shareHash({ ...ticket, model: 'decider-2b' }));
  await again();
  await expect(state).toHaveValue(ticket.state as string);
  await again();
  await expect(state).toHaveValue(ticket.state as string);
  await restorePrevious(page);
  await expect(state).toHaveValue(edited);

  // The drawer open on a phone: Tab from its rows never reaches the skip link; closed, the link is back.
  const own = testInfo.project.use.viewport!;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: 'Open sidebar' }).click();
  await expect(page.getByRole('button', { name: 'Close sidebar' })).toBeFocused();
  const reached: string[] = [];
  for (let step = 0; step < 16; step++) {
    await page.keyboard.press('Tab');
    reached.push(await page.evaluate(() => document.activeElement?.textContent?.trim() ?? ''));
  }
  expect(reached, reached.join(' | ')).not.toContain('Skip to the page');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Open sidebar' })).toBeFocused();
  await expect(page.locator('#sidebar')).toBeHidden();
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('.skip-link')).toBeFocused();
  await page.setViewportSize(own);
  await screenshot(page, testInfo, 'skip_link');
});

// The Share popover closes when the focus moves on beyond it, and Escape closes it wherever the focus is, giving Share
// the focus.
test('share_popover', async ({ page }, testInfo) => {
  await page.goto(`/${shareHash(fixture('ticket.json'))}`);
  const share = page.getByRole('button', { name: 'Share link', exact: true });
  const link = page.getByRole('textbox', { name: 'Link to these inputs' });
  await share.focus();
  await page.keyboard.press('Enter');
  await expect(link).toBeFocused();
  await expect(share).toHaveAttribute('aria-expanded', 'true');
  await screenshot(page, testInfo, 'share_popover', 'open');
  // Tab: the copy key, then on past the popover, which closes behind the focus.
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Copy share link' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(link).toBeHidden();
  await expect(share).toHaveAttribute('aria-expanded', 'false');
  // Open again; a click on its note sends the focus nowhere and leaves it open; Escape then closes it.
  await share.click();
  await expect(link).toBeFocused();
  await page.getByText('Anyone with this link opens these inputs').click();
  await expect(link).toBeVisible();
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
  await page.keyboard.press('Escape');
  await expect(link).toBeHidden();
  await expect(share).toBeFocused();
  await screenshot(page, testInfo, 'share_popover');
});

// A run that comes to the same words as the last is said again: the status region empties for a frame, then says them.
// Loading a preset says so, the same preset twice included.
test('announcements', async ({ page }, testInfo) => {
  const replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await page.goto(`/?autorun=1${shareHash(refused())}`);
  expect((await replied).status()).toBe(422);
  const status = page.getByTestId('run-status');
  await expect(status).toHaveText('HTTP 422, not answered');
  // Every text either region takes from now on, as assistive technology is told of it.
  await page.evaluate(() => {
    const heard: Record<string, string[]> = { 'run-status': [], notice: [] };
    for (const id of Object.keys(heard)) {
      const region = document.querySelector(`[data-testid="${id}"]`)!;
      new MutationObserver(() => heard[id].push(region.textContent ?? '')).observe(region, {
        subtree: true,
        childList: true,
        characterData: true,
      });
    }
    Object.assign(window, { heard });
  });
  const heard = (id: string) => page.evaluate((id) => (window as unknown as { heard: Record<string, string[]> }).heard[id], id);
  expect((await run(page)).status()).toBe(422);
  await expect.poll(() => heard('run-status')).toEqual(['', 'HTTP 422, not answered']);
  await expect(status).toHaveText('HTTP 422, not answered');

  const loaded = 'Loaded the Ticket routing preset';
  await loadPreset(page, 'Ticket routing');
  await expect(page.getByTestId('notice')).toHaveText(loaded);
  await loadPreset(page, 'Ticket routing');
  await expect.poll(async () => (await heard('notice')).filter((text) => text === loaded)).toHaveLength(2);
  await expect(page.getByTestId('notice')).toHaveText(loaded);
  await screenshot(page, testInfo, 'announcements');
});

// When the columns stack (a phone), a run's answers land a screen below Run or the editor: the focus moves to the first
// question's id, or to the fault's title, whole under the bars, after Ctrl+Enter from the state editor and after a
// tapped Run. On a wide screen it stays where it was.
test('run_focus', async ({ page, browser }, testInfo) => {
  const narrow = testInfo.project.use.viewport!.width <= 960;
  const ticket = fixture('ticket.json');
  const firstQuestion = (page: Page) => page.getByTestId('channel').first().getByRole('heading', { level: 3 });
  const faultTitle = (page: Page) => page.getByTestId('fault').locator('.fault-title');
  // Where the focus lands; on a phone, whole under the bars once the page has scrolled there.
  const lands = async (target: Locator) => {
    await expect(target).toBeFocused();
    if (narrow) {
      await expect.poll(() => underTheBars(target)).toBe(true);
    }
  };
  await page.goto(`/${shareHash(ticket)}`);
  const state = page.getByRole('textbox', { name: 'State' });
  await state.focus();
  let replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await page.keyboard.press('Control+Enter');
  expect((await replied).status()).toBe(200);
  await expect(page.getByTestId('answered-by')).toBeVisible();
  const landed = narrow ? firstQuestion(page) : state;
  await lands(landed);
  await screenshot(page, testInfo, 'run_focus', 'ctrl-enter');

  // A run the API refuses: the fault's title on a phone, Run on a wide screen.
  await page.getByRole('textbox', { name: 'Questions JSON' }).fill(JSON.stringify(refused().questions, null, 2));
  replied = page.waitForResponse((r) => isSystemOne(r.url()));
  await runKey(page).click();
  expect((await replied).status()).toBe(422);
  const faulted = narrow ? faultTitle(page) : runKey(page);
  await lands(faulted);
  await screenshot(page, testInfo, 'run_focus', 'fault');

  // Run tapped on a touch screen of the project's size.
  const touch = await browser.newContext({
    baseURL: testInfo.project.use.baseURL,
    viewport: testInfo.project.use.viewport,
    hasTouch: true,
  });
  const tapped = await touch.newPage();
  await tapped.goto(`/${shareHash(ticket)}`);
  await expect(runKey(tapped)).toHaveAttribute('aria-disabled', 'false');
  replied = tapped.waitForResponse((r) => isSystemOne(r.url()));
  await runKey(tapped).tap();
  expect((await replied).status()).toBe(200);
  await expect(tapped.getByTestId('answered-by')).toBeVisible();
  const reached = narrow ? firstQuestion(tapped) : runKey(tapped);
  await lands(reached);
  await screenshot(tapped, testInfo, 'run_focus');
  await touch.close();
});

/** Every id an ARIA relationship or a label names that no element on the page holds. */
function dangling(): string[] {
  const attributes = ['aria-controls', 'aria-describedby', 'aria-labelledby', 'aria-owns', 'aria-details', 'for'];
  const missing: string[] = [];
  for (const element of document.querySelectorAll(attributes.map((a) => `[${a}]`).join(', '))) {
    for (const attribute of attributes) {
      for (const id of (element.getAttribute(attribute) ?? '').split(/\s+/).filter(Boolean)) {
        if (!document.getElementById(id)) {
          missing.push(`${element.tagName.toLowerCase()} ${attribute}="${id}"`);
        }
      }
    }
  }
  return missing;
}

// No relationship names an id that is not on the page, in every state the page takes: empty, a link loaded, the Share
// popover open and closed, answers with the builder closed and open, a model the server has not pulled, a run in the
// tab in flight.
test('aria_relationships', async ({ page }, testInfo) => {
  test.setTimeout(300_000);
  const ticket = fixture('ticket.json');
  const none = async (state: string) => expect(await page.evaluate(dangling), state).toEqual([]);
  await page.goto('/');
  await expect(picker(page)).toHaveValue('decider-2b');
  await none('empty');
  await page.goto(`/${shareHash(ticket)}`);
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  await none('a link loaded, Run free');
  const share = page.getByRole('button', { name: 'Share link', exact: true });
  await share.click();
  await expect(page.getByRole('textbox', { name: 'Link to these inputs' })).toBeVisible();
  await none('the Share popover open');
  await page.keyboard.press('Escape');
  await none('the Share popover closed');
  expect((await run(page)).status()).toBe(200);
  await none('answers, builders closed');
  await channel(page, 'department').getByRole('button', { name: 'Edit question department' }).click();
  await expect(channel(page, 'department').getByRole('textbox', { name: 'Question id' })).toBeVisible();
  await none('a builder open');
  await pickModel(page, 'decider-4b');
  await expect(page.getByTestId('run-note')).toContainText('This server has not pulled decider-4b');
  await none('a model the server has not pulled');
  const cdp = await inFlight(page);
  await none('a run in the tab in flight');
  await screenshot(page, testInfo, 'aria_relationships');
  await stopInFlight(page, cdp);
  await none('the run stopped');
});

// Each fact row is one stop for Tab: its first name, whose tooltip then says how the arrow keys reach the others. No
// tooltip joins a name in Chrome's accessibility tree: Run is "Run" after a run in the tab and after its fault, with its
// tooltip showing, and a fact's name is its own.
test('term_stops', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const ticket = fixture('ticket.json');
  const request: Request = {
    ...ticket,
    model: 'decider-2b',
    questions: {
      department: ticket.questions.department,
      urgency: { type: 'score', instructions: 'How urgent is the ticket?', criteria: ['not urgent', 'soon', 'urgent'] },
    },
  };
  await page.goto(`/?autorun=1${shareHash(request)}`);
  await expect(page.getByTestId('answered-by')).toContainText('Answered by', { timeout: 120_000 });
  // Tab from the state editor to Add question: the names it meets, by question.
  await page.locator('#state').focus();
  const met: string[] = [];
  for (let step = 0; step < 60; step++) {
    await page.keyboard.press('Tab');
    const at = await page.evaluate(() => {
      const focused = document.activeElement!;
      if (focused.id === 'add-question') {
        return null;
      }
      return focused.tagName === 'DFN'
        ? `${focused.closest('[data-question]')!.getAttribute('data-question')}: ${focused.textContent}`
        : '';
    });
    if (at === null) {
      break;
    }
    if (at) {
      met.push(at);
    }
  }
  expect(met).toEqual(['department: Answer', 'urgency: Score']);

  // Along a row: Right, End, Home, Left at the first stays; the entry's tooltip names the keys when the keyboard brings
  // it the focus.
  const names = channel(page, 'department').locator('dfn');
  const tip = (name: Locator) => name.evaluate((element) => getComputedStyle(element, '::after'));
  await page.locator('#state').focus();
  while (!(await names.first().evaluate((name) => name === document.activeElement))) {
    await page.keyboard.press('Tab');
  }
  expect((await tip(names.first())).content).toContain('Left and Right arrows: the other facts');
  await page.keyboard.press('ArrowRight');
  await expect(names.nth(1)).toBeFocused();
  expect((await tip(names.nth(1))).display).toBe('block');
  await page.keyboard.press('End');
  await expect(names.last()).toBeFocused();
  await page.keyboard.press('Home');
  await expect(names.first()).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(names.first()).toBeFocused();
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Accessibility.enable');
  expect(await focusedName(cdp)).toBe('Answer');
  await page.keyboard.press('ArrowRight');
  expect(await focusedName(cdp)).toBe('Confidence');
  await screenshot(page, testInfo, 'term_stops', 'facts');

  // Run, focused by the keyboard with its tooltip showing, after a run in the tab and after a fault there (on a phone
  // once the page has brought the focus to the answers or the fault, as it does after a run there).
  const narrow = testInfo.project.use.viewport!.width <= 960;
  const runFocused = async () => {
    if (narrow) {
      await expect(page.locator('.question-id, .fault-title').first()).toBeFocused();
    }
    await page.locator('#state').focus();
    await page.keyboard.press('Shift+Tab');
    await page.locator('#run-key').focus();
    await expect(page.locator('#run-key')).toBeFocused();
    expect((await tip(page.locator('#run-key'))).display).toBe('block');
    return focusedName(cdp);
  };
  await pickInBrowser(page, 'decider-0.8b');
  await runKey(page).click();
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  expect(await runFocused()).toBe('Run');
  await page.getByRole('textbox', { name: 'Questions JSON' }).fill(JSON.stringify(refused().questions, null, 2));
  await runKey(page).click();
  await expect(page.getByTestId('raw-status')).toHaveText('HTTP 422', { timeout: 600_000 });
  await expect(page.getByTestId('fault')).toBeVisible();
  expect(await runFocused()).toBe('Run');
  await screenshot(page, testInfo, 'term_stops');
});

// What the page adds or takes away keeps the focus with the person: Add option and Add level move it to the new row's
// first field; Stop leaving once the download is over gives it to Run, as Stop's own press does.
test('page_focus', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  await page.goto('/');
  await page.getByRole('button', { name: 'Add question' }).click();
  const q1 = channel(page, 'q1');
  await q1.getByRole('radio', { name: 'choice', exact: true }).check();
  await q1.getByRole('button', { name: 'Add option' }).click();
  await expect(q1.getByRole('textbox', { name: 'Option 3 name' })).toBeFocused();
  // The score keeps the three option names as its levels; the next is level 3.
  await q1.getByRole('radio', { name: 'score', exact: true }).check();
  const levels = await q1.getByRole('textbox', { name: /^Level \d+ description$/ }).count();
  await q1.getByRole('button', { name: 'Add level' }).click();
  await expect(q1.getByRole('textbox', { name: `Level ${levels} description` })).toBeFocused();
  await screenshot(page, testInfo, 'page_focus', 'added');

  // Stop has the focus when the download ends: every control the focus enters from then on, in order.
  const cdp = await inFlight(page, 8);
  await page.evaluate(() => {
    const entered: string[] = [];
    document.addEventListener('focusin', (event) => {
      const target = event.target as HTMLElement;
      entered.push(target.id || target.className);
    });
    Object.assign(window, { entered });
  });
  const stop = page.getByTestId('run-note').getByRole('button', { name: 'Stop' });
  await stop.focus();
  await throttle(cdp, null);
  await expect(stop).toHaveCount(0, { timeout: 120_000 });
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  const entered = await page.evaluate(() => (window as unknown as { entered: string[] }).entered);
  expect(entered[0], entered.join(' | ')).toContain('banner-stop');
  expect(entered[1], entered.join(' | ')).toBe('run-key');
  await screenshot(page, testInfo, 'page_focus');
});
