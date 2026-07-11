import { test, expect, type Page } from '@playwright/test';

async function installOnboardingMock(page: Page) {
  await page.addInitScript(() => {
    let pollResolve: ((value: unknown) => void) | null = null;
    const callbacks = new Map<number, (...args: unknown[]) => void>();
    let callbackId = 0;
    const internals = {
      transformCallback(callback: (...args: unknown[]) => void) {
        const id = ++callbackId;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback(id: number) { callbacks.delete(id); },
      invoke(command: string, args: Record<string, unknown> = {}) {
        if (command === 'get_setting') {
          if (args.key === 'onboarding_complete') return Promise.resolve(false);
          if (args.key === 'modrinth_enabled') return Promise.resolve(true);
          if (args.key === 'ai_mcp_enabled') return Promise.resolve(false);
          if (args.key === 'ai_chat_enabled') return Promise.resolve(true);
          return Promise.resolve(null);
        }
        if (command === 'set_setting') return Promise.resolve(null);
        if (command === 'get_windows_accent_color') return Promise.resolve(null);
        if (command.startsWith('plugin:event|') || command.startsWith('plugin:shell|')) return Promise.resolve(null);
        if (command === 'github_login') {
          return Promise.resolve({
            device_code: 'device',
            user_code: 'ABCD-EFGH',
            verification_uri: 'https://github.com/login/device',
            expires_in: 900,
            interval: 1,
          });
        }
        if (command === 'github_login_poll') {
          return new Promise((resolve) => { pollResolve = resolve; });
        }
        return Promise.resolve(null);
      },
    };
    Object.assign(window as unknown as Record<string, unknown>, {
      __TAURI_INTERNALS__: internals,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
      __resolveGithubPoll(value: unknown) { pollResolve?.(value); },
    });
  });
}

test('app shell renders with Agora branding', async ({ page }) => {
  await page.goto('/');
  // The app should load the home page (Vite app shell).
  await expect(page.locator('body')).toBeVisible();
  // Look for the Agora title text (sidebar <h1>) somewhere on the page.
  await expect(page.getByRole('heading', { name: 'Agora', level: 1 })).toBeVisible({ timeout: 10000 });
});

test('navigation to browse works', async ({ page }) => {
  await page.goto('/');
  // Sidebar uses <button> elements (not <a> links) for navigation.
  const browseButton = page.getByRole('button', { name: 'Browse', exact: true });
  await expect(browseButton).toBeVisible();
  await browseButton.click();
  // After clicking Browse, the page should show a "Browse" heading.
  await expect(page.getByRole('heading', { name: 'Browse', level: 2 })).toBeVisible({ timeout: 5000 });
});

test('sidebar navigation buttons are visible', async ({ page }) => {
  await page.goto('/');
  // All base sidebar tabs should be present as buttons.
  const tabs = ['Home', 'Browse', 'My Instances', 'Community Governance', 'Settings'];
  for (const tab of tabs) {
    await expect(page.getByRole('button', { name: tab, exact: true })).toBeVisible();
  }
});

test('persisted service choices survive Back and Continue', async ({ page }) => {
  await installOnboardingMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Get Started' }).click();
  await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();

  const switches = page.getByRole('switch');
  await expect(switches.nth(0)).toHaveAttribute('aria-checked', 'true');
  await expect(switches.nth(1)).toHaveAttribute('aria-checked', 'false');
  await expect(switches.nth(2)).toHaveAttribute('aria-checked', 'true');

  await switches.nth(1).click();
  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByRole('button', { name: 'Get Started' }).click();
  await expect(page.getByRole('switch').nth(1)).toHaveAttribute('aria-checked', 'true');
});

test('cancelling GitHub device flow invalidates the active poll', async ({ page }) => {
  await installOnboardingMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Get Started' }).click();
  await page.getByRole('button', { name: 'Continue' }).click();
  await page.getByRole('button', { name: 'Sign in with GitHub' }).click();
  await expect(page.getByRole('button', { name: 'Copy Code' })).toBeVisible();

  await page.getByRole('button', { name: 'Cancel' }).click();
  await page.evaluate(() => (window as any).__resolveGithubPoll(true));
  await expect(page.getByRole('heading', { name: 'Connect GitHub' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Download Registry' })).toHaveCount(0, { timeout: 1500 });
});
