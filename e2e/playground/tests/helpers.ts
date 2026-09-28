// Shared pieces of the playground cases: fixtures, Jev share links, the display formats and screenshots.
import fs from 'node:fs';
import path from 'node:path';
import { expect, type Page, type TestInfo } from '@playwright/test';
import LZString from 'lz-string';

export const root = process.env.ARDANA_REPO_ROOT ?? path.resolve(__dirname, '../../..');

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Request = { model?: string; state: Json; questions: Record<string, Json> };

export function fixture(name: string): Request {
  return JSON.parse(fs.readFileSync(path.join(root, 'tests/fixtures/requests', name), 'utf8'));
}

/** The `#share/...` fragment Jev's playground writes for a request. */
export function shareHash(request: Request): string {
  const payload = {
    apiVersion: 'v1',
    documentText: typeof request.state === 'string' ? request.state : JSON.stringify(request.state, null, 2),
    promptsText: JSON.stringify(request.questions, null, 2),
    selectedModels: [request.model ?? 'jev-latest'],
  };
  return `#share/${LZString.compressToEncodedURIComponent(JSON.stringify(payload))}`;
}

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
