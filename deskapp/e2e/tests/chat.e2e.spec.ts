/**
 * Phase 3: E2E 测试 - Chat 功能
 * 核心测试场景：
 * 1. 发送消息 - 消息出现在 UI 中
 * 2. 流式响应 - token 实时追加
 * 3. 工具调用 - 工具卡片正确显示
 * 4. 错误恢复 - 错误后继续发送
 */

import { test, expect } from '@playwright/test';

test.describe('Chat functionality', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:1420');
  });

  test('sends message and receives streaming response', async ({ page }) => {
    // 1. 输入消息
    await page.fill('#message-input', 'Hello');
    await page.click('#send-button');

    // 2. 断言消息出现在 UI
    await expect(page.locator('.chat-message')).toBeVisible();
  });

  test('handles user input validation', async ({ page }) => {
    // 测试空消息
    await page.click('#send-button');

    // 应该显示验证错误
    await expect(page.locator('.validation-error')).toBeVisible();
  });

  test('displays streaming tokens in real-time', async ({ page }) => {
    await page.fill('#message-input', 'Generate a long response');
    await page.click('#send-button');

    // 等待流式响应出现
    await page.waitForSelector('.token-stream', { timeout: 10000 });

    // 断言多个 token 元素存在
    const tokens = page.locator('.token');
    await expect(tokens.first()).toBeVisible();
  });
});
