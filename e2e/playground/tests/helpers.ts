// Shared pieces of the playground cases: fixtures, Jev share links, the display formats and screenshots.
import fs from 'node:fs';
import path from 'node:path';
import { type CDPSession, expect, type Page, type Response, type TestInfo } from '@playwright/test';
import LZString from 'lz-string';

export const root = process.env.ARDANA_REPO_ROOT ?? path.resolve(__dirname, '../../..');

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Request = { model?: string; state: Json; questions: Record<string, Json> };
/** A `/v1/models` entry with Ardana's extras. */
export type ModelInfo = {
  name: string;
  description: string;
  release_date: string;
  x_pulled?: boolean;
  x_default?: boolean;
  x_size?: number;
  x_browser?: number;
  x_browser_pulled?: boolean;
  x_browser_default?: boolean;
};

/** A byte count as Ardana prints model sizes: decimal units, one decimal (`1.3 GB`). */
export function decimalSize(bytes: number): string {
  if (bytes < 1000) {
    return `${bytes} B`;
  }
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1000;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/** A request file, by its path from the repo root. */
export function readRequest(file: string): Request {
  return JSON.parse(fs.readFileSync(path.join(root, file), 'utf8'));
}

export function fixture(name: string): Request {
  return readRequest(`tests/fixtures/requests/${name}`);
}

/** The state and questions editor texts of a request, as Jev writes them into a share link. */
export function editorTexts(request: Request): { documentText: string; promptsText: string } {
  return {
    documentText: typeof request.state === 'string' ? request.state : JSON.stringify(request.state, null, 2),
    promptsText: JSON.stringify(request.questions, null, 2),
  };
}

/** The `#share/...` fragment Jev's playground writes for a request (`jev-latest` unless the request names a model). */
export function shareHash(request: Request): string {
  const payload = { apiVersion: 'v1', ...editorTexts(request), selectedModels: [request.model ?? 'jev-latest'] };
  return `#share/${LZString.compressToEncodedURIComponent(JSON.stringify(payload))}`;
}

/**
 * The Run key, by each name its face takes (Run, and Running, Loading or Pulling while a run is in flight): never a
 * banner action whose words start with "Run", such as the standalone build's "Run decider:0.8b in this tab instead".
 */
export const runKey = (page: Page) => page.getByRole('button', { name: /^(Run|Running|Loading|Pulling)$/ });

/** The state editor. */
export const stateBox = (page: Page) => page.getByRole('textbox', { name: 'State' });

/** The questions editor. */
export const questionsBox = (page: Page) => page.getByRole('textbox', { name: 'Questions JSON' });

/** The body Run sends for `model` while the editors hold `request`'s state and questions, as the page writes it. */
export const body = (model: string, request: Request) =>
  JSON.stringify({ model, state: request.state, questions: request.questions }, null, 2);

/** `text` as a regular expression that matches it from its start. */
export const startsWith = (text: string) => new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`);

/**
 * The model picker and the presets live in the sidebar, which is a drawer behind "Open sidebar" on a phone (and when
 * a wide screen has collapsed it): opens it for `act` when it is closed, and closes it again afterwards.
 */
export async function inSidebar<T>(page: Page, act: () => Promise<T>): Promise<T> {
  const opener = page.getByRole('button', { name: 'Open sidebar' });
  const closed = await opener.isVisible();
  if (closed) {
    await opener.click();
  }
  const result = await act();
  // A pick that loads a preset or jumps to a section closes the drawer itself; only a lingering one is closed here.
  if (closed && (await opener.getAttribute('aria-expanded')) === 'true') {
    await page.getByRole('button', { name: 'Close sidebar' }).click();
  }
  return result;
}

/** Loads a preset from the sidebar. */
export const loadPreset = (page: Page, name: string) =>
  inSidebar(page, () => page.getByRole('button', { name }).click());

/**
 * The model picker, for reading its value and options. It is `#model` rather than a role locator: on a phone the
 * closed drawer is `visibility: hidden`, which takes the select out of the accessibility tree.
 */
export const picker = (page: Page) => page.locator('#model');

/** Picks a model in the sidebar. */
export const pickModel = (page: Page, name: string) =>
  inSidebar(page, () => page.getByRole('combobox', { name: 'Model' }).selectOption(name));

/** The picker's value of a model's "In browser" row: the name and a word no model name holds. */
export const browserRow = (name: string) => `${name} in-browser`;

/** Picks a model's "In browser" row in the sidebar: Run then answers in the tab. */
export const pickInBrowser = (page: Page, name: string) => pickModel(page, browserRow(name));

/** Each answer's top pick: the chosen option, the most probable score level, or whether a noul leans yes. */
export function topAnswers(response: { answers: Record<string, Record<string, Json>> }): Record<string, string> {
  return Object.fromEntries(
    Object.entries(response.answers).map(([id, answer]) => {
      switch (answer.type) {
        case 'choice':
          return [id, answer.choice as string];
        case 'score': {
          const levels = Object.entries(answer.probabilities as Record<string, number>);
          return [id, levels.reduce((a, b) => (b[1] > a[1] ? b : a))[0]];
        }
        default:
          return [id, (answer.noul as number) >= 0.5 ? 'yes' : 'no'];
      }
    }),
  );
}

/** What the page says running a browser model takes in memory, after what its first run downloads. */
export const IN_MEMORY = 'Running it takes 3 to 4 times that in memory';

/** ardana.ai's install line, which the standalone build shows before the ardana command; a server runs ardana. */
export const INSTALL = 'curl -fsSL https://ardana.ai/install.sh | sh';

/**
 * The `ardana run` command the snippets show for a model this server does not run: its command line, and the request
 * the heredoc carries on stdin.
 */
export async function ardanaCommand(page: Page): Promise<{ text: string; line: string; body: string }> {
  const snippet = page.getByTestId('snippet');
  await expect(snippet).toHaveAttribute('data-language', 'ardana');
  const text = (await snippet.textContent())!;
  const end = text.indexOf('\n');
  expect(text.endsWith('\nJSON\n'), text).toBe(true);
  return { text, line: text.slice(0, end), body: text.slice(end + 1, -'\nJSON\n'.length) };
}

/** Every text a person can read on the page: the rendered text, the picker's option and group labels, the title. */
export async function visibleText(page: Page): Promise<string> {
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

/** Puts both editors back as they were before the last preset load or removal, from the sidebar. */
export const restorePrevious = (page: Page) =>
  inSidebar(page, () => page.getByRole('button', { name: 'Restore previous' }).click());

/** Presses RUN and returns the `/v1/systemone` response, however long the model takes (the test timeout bounds it). */
export async function run(page: Page): Promise<Response> {
  const replied = page.waitForResponse((r) => isSystemOne(r.url()), { timeout: 0 });
  await runKey(page).click();
  return replied;
}

/** A question's channel. */
export const channel = (page: Page, id: string) =>
  page.locator(`[data-testid="channel"][data-question="${id.replace(/["\\]/g, '\\$&')}"]`);

/** A JSON pointer from its unescaped parts. */
export const pointer = (...parts: string[]) =>
  parts.map((part) => `/${part.replace(/~/g, '~0').replace(/\//g, '~1')}`).join('');

/**
 * Rounds a JSON number, multiplied by 10^shift, to `places` decimals, half up, on its decimal digits: the rule the
 * playground displays with (percent with one decimal, two decimals otherwise), computed here with BigInt.
 */
export function roundDecimal(value: number, shift: number, places: number): string {
  const [mantissa, exp = '0'] = String(value).toLowerCase().split('e');
  const negative = mantissa.startsWith('-');
  const [intPart, fracPart = ''] = mantissa.replace('-', '').split('.');
  const digits = BigInt(intPart + fracPart);
  const scale = Number(exp) - fracPart.length + shift + places;
  let n: bigint;
  if (scale >= 0) {
    n = digits * 10n ** BigInt(scale);
  } else {
    const unit = 10n ** BigInt(-scale);
    n = digits / unit + ((digits % unit) * 2n >= unit ? 1n : 0n);
  }
  const text = n.toString().padStart(places + 1, '0');
  const whole = text.slice(0, text.length - places);
  const sign = negative && n !== 0n ? '-' : '';
  return places === 0 ? `${sign}${whole}` : `${sign}${whole}.${text.slice(text.length - places)}`;
}

export function formatted(format: string, value: Json): string {
  switch (format) {
    case 'percent':
      return `${roundDecimal(value as number, 2, 1)}%`;
    case 'fixed2':
      return roundDecimal(value as number, 0, 2);
    case 'verbatim':
      return String(value);
    default:
      throw new Error(`unknown data-format ${format}`);
  }
}

/** The value at a JSON pointer. */
export function at(json: Json, pointer: string): Json | undefined {
  let node: Json | undefined = json;
  for (const raw of pointer.split('/').slice(1)) {
    const key = raw.replace(/~1/g, '/').replace(/~0/g, '~');
    node = node !== null && typeof node === 'object' ? (node as Record<string, Json>)[key] : undefined;
  }
  return node;
}

/**
 * Every figure on the page that names a response field: its `data-value` is the field's raw value and its text is
 * that value in its display format. Returns the pointers checked.
 */
export async function expectFiguresMatch(page: Page, response: Json): Promise<string[]> {
  const figures = await page.locator('[data-field]').evaluateAll((els) =>
    els.map((el) => ({
      field: el.getAttribute('data-field')!,
      value: el.getAttribute('data-value')!,
      format: el.getAttribute('data-format')!,
      text: el.textContent!.trim(),
    })),
  );
  for (const figure of figures) {
    const expected = at(response, figure.field);
    expect(expected, `${figure.field} is in the response`).not.toBeUndefined();
    if (typeof expected === 'number') {
      expect(Number(figure.value), `data-value of ${figure.field}`).toBe(expected);
    } else {
      expect(figure.value, `data-value of ${figure.field}`).toBe(String(expected));
    }
    expect(figure.text, `text of ${figure.field}`).toBe(formatted(figure.format, expected!));
  }
  return figures.map((figure) => figure.field);
}

/** A full-page screenshot under `tmp/screens/<case>/`: `<project>.png`, or `<state>-<project>.png` for a named state. */
export async function screenshot(page: Page, testInfo: TestInfo, caseName: string, state?: string) {
  const file = state ? `${state}-${testInfo.project.name}.png` : `${testInfo.project.name}.png`;
  await page.screenshot({
    path: path.join(root, 'tmp/screens', caseName, file),
    fullPage: true,
    animations: 'disabled',
  });
}

export const isSystemOne = (url: string) => new URL(url).pathname === '/v1/systemone';

/**
 * Run, held, sends nothing: neither a click (with `force`: Playwright waits on `aria-disabled` buttons) nor Ctrl+Enter
 * (`requests` holds every request the page made) starts a run or says anything, and it shows no tooltip saying it
 * would.
 */
export async function expectSendsNothing(page: Page, requests: string[]) {
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

/** `html { font-size: 200% }`: the text-size setting the reflow cases enlarge the page's text with. */
export const LARGE_TEXT = 'html { font-size: 200% !important; }';

/** Two painted frames: what the page changes after a key, a resize or a style lands by then. */
export const painted = (page: Page) =>
  page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));

/**
 * Narrows the page's network to `mbps` MB/s down, with `latency` ms on every request, or opens it again (`null`), through
 * Chrome's network emulation.
 */
export async function throttle(cdp: CDPSession, mbps: number | null, latency = 0) {
  await cdp.send('Network.emulateNetworkConditions', {
    offline: false,
    latency,
    downloadThroughput: mbps === null ? -1 : mbps * 1024 * 1024,
    uploadThroughput: -1,
  });
}

/**
 * A run in flight: a server run's answers on the page (decider:2b, the ticket), then decider:0.8b's "In browser" row
 * downloading into the tab at `mbps` MB/s. Returns the CDP session that narrows the network; `stopInFlight` ends it.
 */
export async function inFlight(page: Page, mbps = 2): Promise<CDPSession> {
  await page.goto(`/?autorun=1${shareHash({ ...fixture('ticket.json'), model: 'decider:2b' })}`);
  await expect(page.getByTestId('answered-by')).toContainText('Answered by', { timeout: 120_000 });
  await pickInBrowser(page, 'decider:0.8b');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await throttle(cdp, mbps);
  await runKey(page).click();
  await expect(page.locator('#run-note progress')).toBeVisible();
  return cdp;
}

/** Stops the download `inFlight` started, and opens the network again. */
export async function stopInFlight(page: Page, cdp: CDPSession) {
  await page.getByTestId('run-note').getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByTestId('run-label')).toHaveText('Run');
  await throttle(cdp, null);
}

/** One stop of a focus walk (`walkFocus`). */
export type FocusStop = {
  /** The focused control's name, as a person reads it. */
  name: string;
  /** Its box's height, and how much of it shows below what stays of the top bar and the banner. */
  height: number;
  shown: number;
  /** The room the page scrolls a control into: below what stays of the bars and the 12 px under them. */
  room: number;
  /** A text field: the state or questions editor, or a builder field. */
  field: boolean;
  /**
   * It shows whole when it fits the room (2.4.12, as practice); taller, it fills the room, or, a text field, keeps the
   * browser's place with its caret in view and shows (2.4.11).
   */
  clear: boolean;
};

/**
 * Walks the page's tab order from `from` (Tab, or Shift+Tab with `back`) for `steps` stops, measured as painted after
 * each key, and returns every stop on the page: controls in the top bar, the banner, the sidebar and the skip link are
 * left out, as what stays on screen is measured from them (their bottoms, wherever they are).
 */
export async function walkFocus(page: Page, from: string, back: boolean, steps: number): Promise<FocusStop[]> {
  await page.locator(from).focus();
  const stops: FocusStop[] = [];
  for (let step = 0; step < steps; step++) {
    await page.keyboard.press(back ? 'Shift+Tab' : 'Tab');
    await painted(page);
    const stop = await page.evaluate(() => {
      const focused = document.activeElement;
      if (
        !(focused instanceof HTMLElement) ||
        focused === document.body ||
        focused.closest('.topbar, #run-note, .sidebar, .skip-link')
      ) {
        return null;
      }
      const target = focused.closest('.segment') ?? focused;
      const box = target.getBoundingClientRect();
      const cover = Math.max(
        0,
        document.querySelector('.topbar')!.getBoundingClientRect().bottom,
        document.querySelector('#run-note')!.getBoundingClientRect().bottom,
      );
      const room = innerHeight - cover - 12;
      const shown = Math.min(box.bottom, innerHeight) - Math.max(box.top, cover);
      const whole = box.top >= cover - 0.5 && box.bottom <= innerHeight + 0.5;
      const field = focused instanceof HTMLTextAreaElement || focused instanceof HTMLInputElement;
      return {
        name: (focused.getAttribute('aria-label') || focused.textContent || focused.id || focused.tagName)
          .trim()
          .slice(0, 32),
        height: Math.round(box.height),
        shown: Math.round(Math.max(shown, 0)),
        room: Math.round(room),
        field,
        clear: whole || (box.height > room && (field ? shown > 0.5 : shown >= room - 1)),
      };
    });
    if (stop) {
      stops.push(stop);
    }
  }
  return stops;
}

/**
 * A screenshot of the viewport as it is, at a size other than the project's: `tmp/screens/<case>/<project>-<label>.png`
 * (xtask checks the width of `<project>.png` and `<state>-<project>.png` only).
 */
export async function viewShot(page: Page, testInfo: TestInfo, caseName: string, label: string) {
  await page.screenshot({
    path: path.join(root, 'tmp/screens', caseName, `${testInfo.project.name}-${label}.png`),
    animations: 'disabled',
  });
}
