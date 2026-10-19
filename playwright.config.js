import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests', timeout: 60000, workers: 1,
  use: { headless: true, viewport: {width: 1440, height: 1000}, screenshot: 'only-on-failure', trace: 'retain-on-failure' },
});
