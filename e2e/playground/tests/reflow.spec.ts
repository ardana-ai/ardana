// The page at high zoom and with enlarged text, against the release `ardana` serving decider:2b (see
// playwright.config.ts): what stays on screen while a run in the tab is in flight (`banner_reflow`), and rows that wrap
// rather than run off a phone's screen (`text_reflow`). Zoom is the CSS viewport it leaves (1280x1024 at 400% is
// 320x256), enlarged text the root's font size at 200% (`LARGE_TEXT`). A fact's tooltip opens clear of the in-flight
// banner (`term_tooltips`).
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  LARGE_TEXT,
  fixture,
  inFlight,
  painted,
  pickInBrowser,
  root,
  screenshot,
  shareHash,
  stopInFlight,
  viewShot,
  walkFocus,
} from './helpers';

/** The bottom of what stays on screen of the top bar and the banner, with the page scrolled to its questions. */
async function stuckRegion(page: Page): Promise<{ region: number; viewport: number }> {
  await page.locator('#add-question').evaluate((add) => add.scrollIntoView({ block: 'center' }));
  await painted(page);
  return page.evaluate(() => ({
    region: Math.max(
      0,
      ...['.topbar', '#run-note'].map((bar) => document.querySelector(bar)!.getBoundingClientRect().bottom),
    ),
    viewport: innerHeight,
  }));
}

// WCAG 1.4.10 and 2.4.11 while decider:0.8b downloads into the tab: at 400% zoom, with 200% text on a phone, in
// landscape with 200% text, and at 400% zoom with 200% text, what stays of the top bar and the banner holds at most a
// third of the viewport, Stop stays one Tab from Run and shows when focused, no focus stop is hidden, and an editor that
// fits the room below the bars shows whole.
test('banner_reflow', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const cdp = await inFlight(page);
  // The last question's builder open too, so its fields take the focus on the way back.
  await page.getByRole('button', { name: /^Edit question / }).last().click();
  const own = testInfo.project.use.viewport!;
  const sizes: [number, number, boolean][] = [
    [320, 256, false],
    [320, 640, true],
    [844, 390, true],
    [932, 430, true],
    [320, 256, true],
  ];
  for (const [width, height, large] of sizes) {
    await page.setViewportSize({ width, height });
    const text = large ? await page.addStyleTag({ content: LARGE_TEXT }) : null;
    await painted(page);
    const key = `${width}x${height}${large ? ' with 200% text' : ''}`;
    await expect(page.locator('#run-note progress'), key).toBeVisible();

    const { region, viewport } = await stuckRegion(page);
    expect(region, `${key}: what stays of the bars, of a ${viewport} px viewport`).toBeLessThanOrEqual(viewport / 3 + 0.5);
    await viewShot(page, testInfo, 'banner_reflow', `${width}x${height}${large ? '-text200' : ''}`);

    // Stop: the next stop after Run (named Loading meanwhile), in view and under the pointer once focused.
    await page.locator('#run-key').focus();
    await page.keyboard.press('Tab');
    const stop = page.getByTestId('run-note').getByRole('button', { name: 'Stop' });
    await expect(stop, key).toBeFocused();
    await expect(stop, key).toBeInViewport({ ratio: 1 });
    expect(
      await stop.evaluate((key) => {
        const box = key.getBoundingClientRect();
        return document.elementFromPoint((box.left + box.right) / 2, (box.top + box.bottom) / 2) === key;
      }),
      `${key}: Stop takes a pointer`,
    ).toBe(true);

    // Backwards from Add question over the answers and the builder, forwards from the state editor to Snippets.
    for (const [from, back] of [
      ['#add-question', true],
      ['#state', false],
    ] as const) {
      const stops = await walkFocus(page, from, back, 24);
      const walk = `${key}, ${back ? 'Shift+Tab' : 'Tab'}`;
      expect(stops.length, walk).toBeGreaterThan(0);
      expect(
        stops.filter((s) => !s.clear),
        walk,
      ).toEqual([]);
      expect(
        stops.some((s) => s.field),
        `${walk} reaches an editor`,
      ).toBe(true);
    }
    await text?.evaluate((tag) => tag.remove());
  }
  await page.setViewportSize(own);
  await expect(page.locator('#run-note progress')).toBeVisible();
  await screenshot(page, testInfo, 'banner_reflow');
  await stopInFlight(page, cdp);
});

/** Every box past the viewport's right edge (the outermost of a run), and whether the page scrolls sideways. */
function overflow() {
  const name = (el: Element) =>
    `${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ''}${[...el.classList].map((c) => `.${c}`).join('')}`;
  const past = (el: Element) => {
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && box.right > innerWidth + 0.5;
  };
  const boxes = [...document.querySelectorAll('body *')]
    .filter((el) => !el.closest('.sidebar, .visually-hidden') && getComputedStyle(el).position !== 'fixed')
    .filter((el) => past(el) && !(el.parentElement && past(el.parentElement)))
    .map((el) => `${name(el)} ends at ${Math.round(el.getBoundingClientRect().right)}`);
  return { boxes, sideways: document.documentElement.scrollWidth - innerWidth };
}

// With 200% text from 280 to 480 px, nothing runs past the phone's right edge (the segmented rows wrap, the title's word
// breaks), the page never scrolls sideways, and the snippet's copy key shows whole when it takes the focus: on the
// server's curl snippet and on the ardana command an "In browser" row shows.
test('text_reflow', async ({ page }, testInfo) => {
  test.setTimeout(300_000);
  await page.goto(`/?autorun=1${shareHash({ ...fixture('ticket.json'), model: 'decider:2b' })}`);
  await expect(page.getByTestId('answered-by')).toContainText('Answered by', { timeout: 120_000 });
  const own = testInfo.project.use.viewport!;
  await page.addStyleTag({ content: LARGE_TEXT });
  const widths: [number, number][] = [
    [280, 640],
    [300, 640],
    [320, 640],
    [340, 700],
    [360, 740],
    [375, 812],
    [390, 844],
    [415, 844],
    [430, 932],
    [480, 900],
  ];
  for (const snippet of ['curl', 'ardana']) {
    if (snippet === 'ardana') {
      await pickInBrowser(page, 'decider:0.8b');
    }
    for (const [width, height] of widths) {
      await page.setViewportSize({ width, height });
      await painted(page);
      const key = `${snippet}, ${width} px with 200% text`;
      const { boxes, sideways } = await page.evaluate(overflow);
      expect(boxes, key).toEqual([]);
      expect(sideways, `${key}: sideways scroll`).toBe(0);
      // Focused afresh at each width: a focus that stays put scrolls nothing.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      const copy = page.getByTestId('snippet').locator('xpath=..').getByRole('button', { name: /^Copy / });
      await copy.focus();
      await expect(copy, key).toBeInViewport({ ratio: 1 });
      if (width === 280 || width === 390) {
        await viewShot(page, testInfo, 'text_reflow', `${snippet}-${width}`);
      }
    }
  }
  await page.setViewportSize(own);
  await page.getByTestId('snippet').scrollIntoViewIfNeeded();
  await screenshot(page, testInfo, 'text_reflow');
});

/** The focused fact term's tooltip as painted (its `::after` inside the 6 px transparent border), and the area of the
 * banner it covers. */
function tipOverBanner() {
  const term = document.activeElement as HTMLElement;
  const after = getComputedStyle(term, '::after');
  if (after.display === 'none') {
    return { term: term.textContent, shown: false, covered: 0 };
  }
  const box = term.getBoundingClientRect();
  const shift = new DOMMatrixReadOnly(after.transform === 'none' ? undefined : after.transform);
  const left = box.left + parseFloat(after.left) + shift.m41;
  const top = box.top + parseFloat(after.top) + shift.m42;
  const border = parseFloat(after.borderTopWidth);
  const tip = {
    left: left + border,
    top: top + border,
    right: left + parseFloat(after.width) - border,
    bottom: top + parseFloat(after.height) - border,
  };
  const banner = document.querySelector('#run-note')!.getBoundingClientRect();
  const across = Math.max(0, Math.min(tip.right, banner.right) - Math.max(tip.left, banner.left));
  const down = Math.max(0, Math.min(tip.bottom, banner.bottom) - Math.max(tip.top, banner.top));
  return { term: term.textContent, shown: true, covered: Math.round(across * down) };
}

// While decider:0.8b downloads into the tab, a fact term the keyboard brings up (Shift+Tab, which lands it just under the
// banner, then the arrow keys along its row) opens its meaning clear of the banner: at 390, 430 and 1280 px, and at 390
// px with 200% text.
test('term_tooltips', async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const cdp = await inFlight(page);
  const own = testInfo.project.use.viewport!;
  const sizes: [number, number, boolean][] = [
    [390, 844, false],
    [430, 932, false],
    [1280, 800, false],
    [390, 844, true],
  ];
  for (const [width, height, large] of sizes) {
    await page.setViewportSize({ width, height });
    const text = large ? await page.addStyleTag({ content: LARGE_TEXT }) : null;
    await painted(page);
    const key = `${width}x${height}${large ? ' with 200% text' : ''}`;
    const tips: ReturnType<typeof tipOverBanner>[] = [];
    await page.locator('#add-question').focus();
    for (let step = 0; step < 16 && !(await page.locator('#state').evaluate((s) => s === document.activeElement)); step++) {
      await page.keyboard.press('Shift+Tab');
      await painted(page);
      if (await page.evaluate(() => document.activeElement?.tagName === 'DFN')) {
        tips.push(await page.evaluate(tipOverBanner));
        for (let along = 0; along < 4; along++) {
          await page.keyboard.press('ArrowRight');
          await painted(page);
          tips.push(await page.evaluate(tipOverBanner));
        }
        await page.keyboard.press('Home');
      }
    }
    expect(tips.filter((tip) => tip.shown).length, key).toBeGreaterThan(0);
    expect(
      tips.filter((tip) => tip.covered > 0),
      key,
    ).toEqual([]);
    await text?.evaluate((tag) => tag.remove());
  }
  await page.setViewportSize(own);
  await painted(page);
  await page.locator('#add-question').focus();
  while (!(await page.evaluate(() => document.activeElement?.tagName === 'DFN'))) {
    await page.keyboard.press('Shift+Tab');
  }
  await painted(page);
  await page.screenshot({ path: path.join(root, 'tmp/screens/term_tooltips', `${testInfo.project.name}.png`) });
  await stopInFlight(page, cdp);
});
