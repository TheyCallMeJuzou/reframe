import { defineConfig } from '@playwright/test';

// The README's screenshots and GIFs (npm run screenshots), not part of the tests.
export default defineConfig({
  testDir: 'screenshots',
  timeout: 300_000,
  workers: 1,
  reporter: 'list',
});
