import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  timeout: 120_000,
  workers: 1,
  // On GitHub, failures also become annotations of the job, which anyone can read (its log needs admin rights).
  reporter: process.env.GITHUB_ACTIONS ? [['list'], ['github']] : 'list',
});
