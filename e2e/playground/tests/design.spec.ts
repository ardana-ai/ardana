// The design suite's in-tab states (`cargo xtask e2e design` runs only `@design` tests; `cargo xtask e2e playground`
// leaves them out). The real page runs decider:0.8b's browser variant on the design server; the moment its download
// passes a quarter, and again once the answers are drawn, the DOM is cloned and written out as one self-contained
// page (no script; the stylesheets as served, their fonts inlined) that `impeccable detect` renders by file URL at the
// same viewport. A rendered scan of the live URL cannot catch these states: a download holds the network busy, and the
// answers arrive after it goes idle.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { loadPreset, pickInBrowser, runKey } from './helpers';

/** Installs, in the page, `window.freeze(name)`: a clone of the document as it is now, form state included. */
async function installFreezer(page: import('@playwright/test').Page) {
  await page.evaluate(() => {
    const frozen: Record<string, Element> = {};
    const freeze = (name: string) => {
      const clone = document.documentElement.cloneNode(true) as Element;
      // Typed text, picks and checks live in properties, which a clone does not carry: write them into the markup.
      const live = document.querySelectorAll('textarea, input, select');
      const copies = clone.querySelectorAll('textarea, input, select');
      live.forEach((element, i) => {
        const copy = copies[i];
        if (element instanceof HTMLTextAreaElement) {
          copy.textContent = element.value;
        } else if (element instanceof HTMLSelectElement) {
          [...(copy as HTMLSelectElement).options].forEach((option, j) =>
            option.toggleAttribute('selected', j === element.selectedIndex),
          );
        } else if (element instanceof HTMLInputElement && ['radio', 'checkbox'].includes(element.type)) {
          copy.toggleAttribute('checked', element.checked);
        } else if (element instanceof HTMLInputElement) {
          copy.setAttribute('value', element.value);
        }
      });
      frozen[name] = clone;
    };
    Object.assign(window, { frozen, freeze });
    // The download state: frozen inside the mutation that moves the bar past a quarter, before the page paints again.
    new MutationObserver(() => {
      const bar = document.querySelector<HTMLProgressElement>('#run-note progress');
      if (!frozen.download && bar && bar.value / bar.max >= 0.25) {
        freeze('download');
      }
    }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
  });
}

/** A frozen state as one page: no script or link left, the served stylesheets inline with their fonts as data. */
function serialize(name: string): Promise<string> {
  return (async () => {
    const clone = (window as unknown as { frozen: Record<string, Element> }).frozen[name];
    clone.querySelectorAll('script, link').forEach((element) => element.remove());
    let css = '';
    for (const link of document.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')) {
      css += `${await (await fetch(link.href)).text()}\n`;
    }
    for (const url of new Set([...css.matchAll(/url\("([^"]+)"\)/g)].map((match) => match[1]))) {
      const blob = await (await fetch(url)).blob();
      const data = await new Promise<string>((resolve) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result as string);
        reader.readAsDataURL(blob);
      });
      css = css.replaceAll(`url("${url}")`, `url("${data}")`);
    }
    const style = document.createElement('style');
    style.textContent = css;
    clone.querySelector('head')!.append(style);
    return `<!doctype html>\n${clone.outerHTML}\n`;
  })();
}

test('browser_states', { tag: '@design' }, async ({ page }, testInfo) => {
  test.setTimeout(600_000);
  const out = process.env.ARDANA_DESIGN_OUT;
  expect(out, 'cargo xtask e2e design passes the directory the states go to').toBeTruthy();
  await page.goto('/');
  await pickInBrowser(page, 'decider:0.8b');
  await loadPreset(page, 'Ticket routing');
  await installFreezer(page);
  await runKey(page).click();
  await expect(page.getByTestId('answered-by')).toContainText(' in this tab on ', { timeout: 600_000 });
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  expect(await page.evaluate(() => 'download' in (window as unknown as { frozen: object }).frozen)).toBe(true);
  await page.evaluate(() => (window as unknown as { freeze(name: string): void }).freeze('results'));
  for (const state of ['download', 'results']) {
    const html = await page.evaluate(serialize, state);
    fs.writeFileSync(path.join(out!, `browser-${state}-${testInfo.project.name}.html`), html);
  }
});
