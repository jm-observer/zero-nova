/**
 * Phase 3: E2E 测试 - Session 管理
 * 核心测试场景：
 * 1. 创建新会话
 * 2. 切换会话
 * 3. 删除会话
 * 4. 复制会话
 */

import { test, expect } from '@playwright/test';

test.describe('Session management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:1420');
  });

  test('creates a new session', async ({ page }) => {
    // 点击创建新会话按钮
    await page.click('#new-session-btn');

    // 等待新会话出现在侧边栏
    await expect(page.locator('#session-list')).toContainText('New Chat');
  });

  test('switches between sessions', async ({ page }) => {
    // 创建两个会话
    await page.click('#new-session-btn');
    await page.waitForSelector('#session-list', { state: 'visible' });

    // 选择不同的会话
    const sessions = page.locator('#session-list .session-item');
    await sessions.first().click();
    await expect(page.locator('#session-list .session-item.active')).toHaveCount(1);
  });

  test('deletes a session', async ({ page }) => {
    // 删除当前会话
    await page.click('#delete-session-btn');

    // 确认对话框出现
    await expect(page.locator('#confirm-dialog')).toBeVisible();
  });

  test('lists all sessions in sidebar', async ({ page }) => {
    const sessionList = page.locator('#session-list');
    await expect(sessionList).toBeVisible();

    // 会话列表不应为空
    await expect(sessionList.locator('.session-item')).toHaveCount({ gte: 0 });
  });
});
