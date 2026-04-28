import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig, devices } from '@playwright/test';

/**
 * Phase 3: E2E 测试配置
 * 使用 Playwright 测试 Desktop 应用（Tauri/WebView）
 */

const DEV_SERVER_PORT = 1420;
const BASE_URL = `http://127.0.0.1:${DEV_SERVER_PORT}`;
const CONFIG_DIR = path.dirname(fileURLToPath(import.meta.url));
const DESKAPP_ROOT = path.resolve(CONFIG_DIR, '..');

export default defineConfig({
  testDir: './tests',
  timeout: 30000,
  expect: {
    timeout: 5000,
  },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'html',

  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],

  webServer: {
    command: 'pnpm.cmd exec vite --host 127.0.0.1 --port 1420 --strictPort',
    cwd: DESKAPP_ROOT,
    port: DEV_SERVER_PORT,
    reuseExistingServer: !process.env.CI,
  },
});
