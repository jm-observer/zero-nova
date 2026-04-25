/**
 * Phase 3: E2E 测试 - Agent 切换
 * 核心测试场景：
 * 1. 加载 Agent 列表
 * 2. 切换 Agent
 * 3. 创建新 Agent
 */

import { test, expect } from '@playwright/test';

test.describe('Agent switching', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:1420');
  });

  test('displays available agents', async ({ page }) => {
    await expect(page.locator('#agent-list')).toBeVisible();

    // 至少有一个 Agent
    await expect(page.locator('#agent-list .agent-item')).toHaveCount({ gte: 1 });
  });

  test('switches to a different agent', async ({ page }) => {
    // 点击另一个 Agent
    const agentList = page.locator('#agent-list .agent-item');
    if (await agentList.count() > 1) {
      await agentList.nth(1).click();
    }

    // 当前 Agent 应被更新
    await expect(page.locator('#agent-list .agent-item.active')).toHaveCount(1);
  });

  test('creates a new agent', async ({ page }) => {
    // 点击创建 Agent 按钮
    await page.click('#new-agent-btn');

    // 等待创建对话框出现
    await expect(page.locator('#agent-modal')).toBeVisible();
  });
});
