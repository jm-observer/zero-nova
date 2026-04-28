# Deskapp E2E

This directory contains Playwright-based end-to-end tests for the Deskapp web shell.

## Run from `deskapp`

```bash
pnpm install
pnpm exec playwright install chromium
pnpm test:e2e
```

## Notes

- `playwright.config.ts` starts `pnpm dev` automatically.
- Tests should navigate with relative paths such as `page.goto('/')`.
- Current coverage targets the web layer first; Tauri process orchestration is out of scope for this plan.
