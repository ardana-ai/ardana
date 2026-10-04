// The last pass over the page, against the release `ardana` serving decider-2b (see playwright.config.ts): every
// control a finger can reach (`touch_targets`), the current "On this page" row in every scheme (`current_row`), and
// what a first load carries for the shortcut glyphs and the logo (`page_weight`).
import fs from 'node:fs';
import path from 'node:path';
import { type Browser, expect, type Page, test, type TestInfo } from '@playwright/test';
import {
  fixture,
  inFlight,
  root,
  runKey,
  screenshot,
  shareHash,
  stopInFlight,
} from './helpers';

/**
 * The controls in `scope` a finger can miss: from each one's centre the points 21.5 px away (the corners and sides of a
 * 43 px square) must land on it, on its label or on what it holds. Each control is first brought to the middle of the
 * window, clear of the bars.
 */
function shortReach(scope: string): string[] {
  const name = (element: Element) =>
    `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ''}${[...element.classList].map((c) => `.${c}`).join('')}` +
    ` "${(element.getAttribute('aria-label') ?? element.textContent ?? '').trim().slice(0, 24)}"`;
  const controls = [...document.querySelectorAll(`${scope} :is(button, a[href], select, input, textarea, summary)`)].filter(
    (control) => {
      const box = control.getBoundingClientRect();
      return box.width > 0 && box.height > 0 && getComputedStyle(control).visibility !== 'hidden';
    },
  );
  const short: string[] = [];
  for (const control of controls) {
    if (!control.closest('.topbar, #run-note')) {
      control.scrollIntoView({ block: 'center', inline: 'center' });
    }
    const box = control.getBoundingClientRect();
    const [x, y] = [(box.left + box.right) / 2, (box.top + box.bottom) / 2];
    const own = control.closest('label') ?? control;
    for (const [dx, dy] of [
      [-21.5, -21.5],
      [0, -21.5],
      [21.5, -21.5],
      [-21.5, 0],
      [21.5, 0],
      [-21.5, 21.5],
      [0, 21.5],
      [21.5, 21.5],
    ]) {
      const hit = document.elementFromPoint(x + dx, y + dy);
      if (!hit || !own.contains(hit)) {
        short.push(`${name(control)}: ${Math.round(box.width)}x${Math.round(box.height)}, (${dx}, ${dy}) lands on ${hit ? name(hit) : 'nothing'}`);
        break;
      }
    }
  }
  return short;
}

/** The drawn size of each of a few small controls, and whether anything reaches past it (a `::before` box). */
function drawnSizes() {
  const size = (selector: string) => {
    const control = document.querySelector(selector);
    if (!control) {
      return null;
    }
    const box = control.getBoundingClientRect();
    return {
      height: Math.round(box.height),
      reach: getComputedStyle(control, '::before').content !== 'none',
    };
  };
  return {
    edit: size('[data-testid="channel"] .question-controls .button-sm'),
    segment: size('.segment'),
    share: size('#share-button'),
    run: size('#run-key'),
    copy: size('.snippet-box .copy-key'),
    field: size('#questions'),
  };
}

/** The page holding what every kind of control looks like: answers, a builder with its options, the Share popover. */
async function everyControl(page: Page) {
  await page.goto(`/?autorun=1${shareHash({ ...fixture('ticket.json'), model: 'decider-2b' })}`);
  await expect(page.getByTestId('answered-by')).toContainText('Answered by', { timeout: 120_000 });
  await page.getByRole('button', { name: 'Edit question department' }).click();
  await expect(page.getByRole('button', { name: 'Add option' })).toBeVisible();
}

/** Every control a finger can miss, the page's, the sidebar's, the Share popover's, and the banner's actions. */
async function fingerReach(browser: Browser, testInfo: TestInfo): Promise<string[]> {
  const context = await browser.newContext({
    baseURL: testInfo.project.use.baseURL,
    viewport: testInfo.project.use.viewport,
    hasTouch: true,
  });
  const page = await context.newPage();
  try {
    await everyControl(page);
    expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(true);
    const short = await page.evaluate(shortReach, 'body > :not(.skip-link)');
    // The sidebar: a drawer on a phone, open for the measure.
    const opener = page.getByRole('button', { name: 'Open sidebar' });
    if (await opener.isVisible()) {
      await opener.tap();
      await expect(page.getByRole('button', { name: 'Close sidebar' })).toBeFocused();
      // Measured once the drawer has slid in.
      await expect.poll(() => page.locator('#sidebar').evaluate((sidebar) => sidebar.getBoundingClientRect().left)).toBe(0);
      short.push(...(await page.evaluate(shortReach, '.sidebar')));
      await page.keyboard.press('Escape');
      await expect(page.locator('#sidebar')).toBeHidden();
    }
    await page.getByRole('button', { name: 'Share link', exact: true }).tap();
    await expect(page.getByRole('textbox', { name: 'Link to these inputs' })).toBeVisible();
    short.push(...(await page.evaluate(shortReach, '#share-panel')));
    await page.keyboard.press('Escape');
    await screenshot(page, testInfo, 'touch_targets', 'finger');
    // "Show the command", for a model the server has not pulled.
    await page.goto(`/${shareHash({ ...fixture('ticket.json'), model: 'decider-4b' })}`);
    await expect(page.getByRole('button', { name: 'Show the command' })).toBeVisible();
    short.push(...(await page.evaluate(shortReach, '#run-note')));
    // Stop, while decider-0.8b downloads into the tab.
    const cdp = await inFlight(page);
    short.push(...(await page.evaluate(shortReach, '#run-note')));
    await screenshot(page, testInfo, 'touch_targets', 'stop');
    await stopInFlight(page, cdp);
    return short;
  } finally {
    await context.close();
  }
}

// Under a finger (a touch screen of the project's size: 390 px on the phone project, 1280 px on the desktop one) every
// control reaches 44 by 44 px, Stop and "Show the command" included (WCAG 2.5.5); under a mouse the controls keep their
// drawn sizes and reach no further.
test('touch_targets', async ({ page, browser }, testInfo) => {
  test.setTimeout(600_000);
  await everyControl(page);
  expect(await page.evaluate(() => matchMedia('(pointer: fine)').matches)).toBe(true);
  expect(await page.evaluate(drawnSizes)).toEqual({
    edit: { height: 28, reach: false },
    segment: { height: 28, reach: false },
    share: { height: 32, reach: false },
    run: { height: 36, reach: false },
    copy: { height: 40, reach: false },
    field: { height: 200, reach: false },
  });
  await screenshot(page, testInfo, 'touch_targets');
  expect(await fingerReach(browser, testInfo)).toEqual([]);
});

// The section in view stands apart from the other "On this page" rows by its ink and its weight, so it still does in the
// dark scheme, where the ink alone is 2.2:1 against the gray, and in forced colours, which draw every row alike.
test('current_row', async ({ page }, testInfo) => {
  await page.goto(`/${shareHash(fixture('ticket.json'))}`);
  const opener = page.getByRole('button', { name: 'Open sidebar' });
  await expect(page.locator('#model')).toHaveValue('decider-2b');
  const drawer = await opener.isVisible();
  for (const [scheme, forced] of [
    ['light', 'none'],
    ['dark', 'none'],
    ['light', 'active'],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme, forcedColors: forced });
    if (drawer) {
      await opener.click();
    }
    const rows = page.getByRole('group', { name: 'On this page' }).getByRole('button');
    await expect(rows).toHaveCount(3);
    const drawn = await rows.evaluateAll((buttons) =>
      buttons.map((row) => ({
        current: row.getAttribute('aria-current') === 'true',
        weight: getComputedStyle(row).fontWeight,
      })),
    );
    const key = `${scheme}${forced === 'active' ? ', forced colours' : ''}`;
    expect(drawn.filter((row) => row.current), key).toEqual([{ current: true, weight: '500' }]);
    expect(
      drawn.filter((row) => !row.current).map((row) => row.weight),
      key,
    ).toEqual(['400', '400']);
    await screenshot(page, testInfo, 'current_row', `${scheme}${forced === 'active' ? '-forced' : ''}`);
    if (drawer) {
      await page.keyboard.press('Escape');
      await expect(page.locator('#sidebar')).toBeHidden();
    }
  }
});

// A cold load fetches none of Onest's math and symbols subsets (the shortcuts are spelled out: Cmd, Enter), also with
// every shortcut on screen: the sidebar's foot and the tooltips of Run and of the sidebar's own key. The logo's drawing
// is in the page once, a symbol both logos use.
test('page_weight', async ({ page }, testInfo) => {
  const fonts: string[] = [];
  page.on('request', (request) => {
    const file = new URL(request.url()).pathname;
    if (file.startsWith('/fonts/')) {
      fonts.push(file);
    }
  });
  await page.goto(`/${shareHash(fixture('ticket.json'))}`);
  await expect(runKey(page)).toHaveAttribute('aria-disabled', 'false');
  const opener = page.getByRole('button', { name: 'Open sidebar' });
  if (await opener.isVisible()) {
    await opener.click();
  }
  await expect(page.locator('.sidebar-foot')).toContainText('Ctrl+Enter');
  // The sidebar's own key, then Run, each with its tooltip, by the keyboard: Tab to the model picker and back, inside
  // the page (Shift+Tab from the sidebar's first key leaves the document, and where the next Tab lands depends on when
  // the window takes the focus back, which the page then also answers by listing its models again).
  await page.getByRole('button', { name: 'Close sidebar' }).focus();
  await page.keyboard.press('Tab');
  await expect(page.locator('#model')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.getByRole('button', { name: 'Close sidebar' })).toBeFocused();
  if (await opener.isVisible()) {
    await page.keyboard.press('Escape');
    await expect(page.locator('#sidebar')).toBeHidden();
  }
  await page.locator('#run-key').focus();
  expect(await page.locator('#run-key').evaluate((run) => getComputedStyle(run, '::after').display)).toBe('block');
  await page.evaluate(() => document.fonts.ready);
  expect(fonts.length).toBeGreaterThan(0);
  expect(fonts.filter((file) => /\/onest-(math|symbols)-/.test(file))).toEqual([]);

  // The lockup's first path, as the brand kit draws it, is in the page once; both logos use its symbol.
  const lockup = fs.readFileSync(path.join(root, 'crates/ardana-playground/brand/ardana-logo.svg'), 'utf8');
  const first = /<path d="([^"]+)"/.exec(lockup)![1];
  expect(
    await page.evaluate((d) => [...document.querySelectorAll('path')].filter((p) => p.getAttribute('d') === d).length, first),
  ).toBe(1);
  const logos = page.locator('.logo');
  await expect(logos).toHaveCount(2);
  for (const logo of await logos.all()) {
    await expect(logo.locator('use')).toHaveAttribute('href', '#ardana-lockup');
    // Each lockup leads to the site, as the landing's nav logo does.
    await expect(logo).toHaveAttribute('href', 'https://ardana.ai');
    await expect(logo).toHaveAttribute('aria-label', 'Ardana home');
  }
  // The top bar's nav, the landing's: Home, then this page; a phone's bar keeps the lockup alone.
  const links = page.getByRole('navigation', { name: 'Main' }).getByRole('link');
  if (testInfo.project.use.viewport!.width > 720) {
    await expect(links).toHaveText(['Home', 'Playground']);
    await expect(links.first()).toHaveAttribute('href', 'https://ardana.ai');
    await expect(links.last()).toHaveAttribute('aria-current', 'page');
    await expect(links.last()).toHaveAttribute('href', './');
  } else {
    await expect(links).toHaveCount(0);
  }
  const shown = page.locator('.logo:visible');
  await expect(shown).toHaveCount(1);
  await expect(shown).toHaveAccessibleName('Ardana home');
  const box = (await shown.boundingBox())!;
  expect(Math.round(box.height)).toBeGreaterThanOrEqual(18);
  expect(box.width / box.height).toBeCloseTo(610.71 / 106, 1);
  await screenshot(page, testInfo, 'page_weight');
});
