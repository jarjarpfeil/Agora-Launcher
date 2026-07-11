import { test, expect, type Page } from '@playwright/test';

type BrowseResult = { items: unknown[]; total: number; page: number; hasMore: boolean };

const item = (id: string, name: string) => ({
  id,
  source: 'curated',
  registryItem: {
    id,
    name,
    content_type: 'mod',
    download_strategy: 'github_release',
    upvotes: 0,
    downvotes: 0,
    net_score: 0,
  },
  modrinthResult: null,
  name,
  iconUrl: null,
  description: null,
  contentType: 'mod',
});

async function installBrowseMock(page: Page) {
  await page.addInitScript(() => {
    const calls: Array<{
      command: string;
      args: Record<string, unknown>;
      resolve: (value: unknown) => void;
      reject: (reason?: unknown) => void;
    }> = [];
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
          const key = args.key;
          if (key === 'onboarding_complete') return Promise.resolve(true);
          return Promise.resolve(false);
        }
        if (command === 'get_registry_status') {
          return Promise.resolve({
            has_cached_db: true,
            cached_tag: 'test',
            cached_schema_version: 5,
            latest_tag: 'test',
            update_available: false,
            checked: true,
            message: 'Registry ready.',
          });
        }
        if (command === 'list_categories' || command === 'list_manifest_loaders' || command === 'list_manifest_mc_versions') {
          return Promise.resolve([]);
        }
        if (command === 'get_windows_accent_color') return Promise.resolve(null);
        if (command === 'list_instances') return Promise.resolve([]);
        if (command === 'list_snapshots') return Promise.resolve([]);
        if (command.startsWith('plugin:event|')) return Promise.resolve(1);
        if (command === 'browse_search' || command === 'browse_load_more') {
          return new Promise((resolve, reject) => calls.push({ command, args, resolve, reject }));
        }
        return Promise.resolve(null);
      },
    };
    Object.assign(window as unknown as Record<string, unknown>, {
      __TAURI_INTERNALS__: internals,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
      __browseCalls: calls,
      __resolveBrowse(index: number, value: unknown) { calls[index].resolve(value); },
      __rejectBrowse(index: number, value: unknown) { calls[index].reject(value); },
    });
  });
}

async function waitForCalls(page: Page, count: number) {
  await expect.poll(() => page.evaluate(() => (window as any).__browseCalls.length)).toBeGreaterThanOrEqual(count);
}

async function findCall(page: Page, command: string, query?: string, excluded: number[] = []) {
  let index = -1;
  await expect.poll(async () => {
    index = await page.evaluate(({ command, query, excluded }) => {
      const calls = (window as any).__browseCalls as Array<{ command: string; args: Record<string, unknown> }>;
      return calls.findIndex((call, i) =>
        !excluded.includes(i)
        && call.command === command
        && (query === undefined || call.args.query === query),
      );
    }, { command, query, excluded });
    return index;
  }).toBeGreaterThanOrEqual(0);
  return index;
}

async function resolveCall(page: Page, index: number, result: BrowseResult) {
  await page.evaluate(({ index, result }) => (window as any).__resolveBrowse(index, result), { index, result });
}

async function openBrowse(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Browse', exact: true }).click();
  // React StrictMode intentionally runs mount effects twice in development.
  await waitForCalls(page, 2);
  return page.evaluate(() => (window as any).__browseCalls.length - 1) as Promise<number>;
}

test('out-of-order searches only display the newest query', async ({ page }) => {
  await installBrowseMock(page);
  const initial = await openBrowse(page);
  await resolveCall(page, initial, { items: [item('initial', 'Initial')], total: 1, page: 0, hasMore: false });

  const search = page.getByPlaceholder('Search mods, packs, and more…');
  await search.fill('alpha');
  const alpha = await findCall(page, 'browse_search', 'alpha');
  await search.fill('beta');
  const beta = await findCall(page, 'browse_search', 'beta');

  await resolveCall(page, beta, { items: [item('beta', 'Beta Result')], total: 1, page: 0, hasMore: false });
  await expect(page.getByText('Beta Result')).toBeVisible();
  await resolveCall(page, alpha, { items: [item('alpha', 'Alpha Result')], total: 1, page: 0, hasMore: false });

  await expect(page.getByText('Beta Result')).toBeVisible();
  await expect(page.getByText('Alpha Result')).toHaveCount(0);
});

test('stale pagination is ignored and new query can paginate', async ({ page }) => {
  await installBrowseMock(page);
  const initial = await openBrowse(page);
  await resolveCall(page, initial, { items: [item('a', 'Query A')], total: 40, page: 0, hasMore: true });
  const staleLoad = await findCall(page, 'browse_load_more');

  const search = page.getByPlaceholder('Search mods, packs, and more…');
  await search.fill('beta');
  const beta = await findCall(page, 'browse_search', 'beta');
  await resolveCall(page, beta, { items: [item('b', 'Query B')], total: 40, page: 0, hasMore: true });
  await resolveCall(page, staleLoad, { items: [item('a-more', 'Stale A Page')], total: 40, page: 1, hasMore: false });

  const betaLoad = await findCall(page, 'browse_load_more', undefined, [staleLoad]);
  const args = await page.evaluate((index) => (window as any).__browseCalls[index].args, betaLoad);
  expect(args.queryKey).toContain('beta');
  await resolveCall(page, betaLoad, { items: [item('b', 'Query B'), item('b-more', 'Query B Page')], total: 40, page: 1, hasMore: false });

  await expect(page.getByText('Stale A Page')).toHaveCount(0);
  await expect(page.getByText('Query B Page')).toBeVisible();
  await expect(page.getByText('Query B', { exact: true })).toHaveCount(1);
});

test('pagination failure is visible and retryable', async ({ page }) => {
  await installBrowseMock(page);
  const initial = await openBrowse(page);
  await resolveCall(page, initial, { items: [item('a', 'Initial Page')], total: 40, page: 0, hasMore: true });
  const failedLoad = await findCall(page, 'browse_load_more');
  await page.evaluate((index) => (window as any).__rejectBrowse(index, new Error('Pagination failed')), failedLoad);

  await expect(page.getByText('Pagination failed')).toBeVisible();
  await page.getByRole('button', { name: 'Retry loading more' }).click();
  const retry = await findCall(page, 'browse_load_more', undefined, [failedLoad]);
  await resolveCall(page, retry, { items: [item('more', 'Next Page')], total: 40, page: 1, hasMore: false });
  await expect(page.getByText('Next Page')).toBeVisible();
  await expect(page.getByText('Pagination failed')).toHaveCount(0);
});
