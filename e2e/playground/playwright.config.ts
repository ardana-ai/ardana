// The playground suites (`cargo xtask e2e playground`). xtask starts the release `ardana serve` on decider-2b and
// passes its URL in `ARDANA_BASE_URL`; every output lands under the repo's `tmp/`.
import path from 'node:path';
import { defineConfig } from '@playwright/test';

const root = process.env.ARDANA_REPO_ROOT ?? path.resolve(__dirname, '../..');
const baseURL = process.env.ARDANA_BASE_URL;
if (!baseURL) {
  throw new Error('ARDANA_BASE_URL is unset: run the suite with `cargo xtask e2e playground`');
}

export default defineConfig({
  testDir: './tests',
  outputDir: path.join(root, 'tmp/playwright/results'),
  reporter: [
    ['list'],
    ['html', { outputFolder: path.join(root, 'tmp/playwright/report'), open: 'never' }],
    ['json', { outputFile: path.join(root, 'tmp/playwright/results.json') }],
  ],
  // One decode at a time on the server: parallel cases would skew latency and could meet a 503.
  workers: 1,
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  timeout: 180_000,
  expect: { timeout: 60_000 },
  use: {
    baseURL,
    channel: 'chrome',
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { channel: 'chrome', viewport: { width: 1280, height: 800 } } },
    { name: 'mobile', use: { channel: 'chrome', viewport: { width: 390, height: 844 } } },
  ],
});
