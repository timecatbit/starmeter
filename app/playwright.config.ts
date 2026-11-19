import { defineConfig, devices } from '@playwright/test';

// The panel's UI against the Vite dev server with the synthetic fixtures. WebKit is the engine closest to the macOS
// app's WKWebView; install it once with `npx playwright install webkit`. PLAYWRIGHT_BASE_URL points the tests at a dev
// server that is already running instead of starting one.
const given = process.env.PLAYWRIGHT_BASE_URL;
const baseURL = given ?? 'http://127.0.0.1:1431';

export default defineConfig({
  testDir: 'tests',
  timeout: 30_000,
  fullyParallel: true,
  reporter: [['list']],
  projects: [{ name: 'webkit', use: { ...devices['Desktop Safari'], baseURL, viewport: { width: 700, height: 1700 }, deviceScaleFactor: 1 } }],
  webServer: given ? undefined : { command: 'npx vite --host 127.0.0.1 --port 1431 --strictPort', url: baseURL, reuseExistingServer: true, timeout: 60_000 },
});
