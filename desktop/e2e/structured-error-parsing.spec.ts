import { test, expect } from '@playwright/test';
import { parseLauncherError } from '../src/lib/tauri';

test.describe('parseLauncherError structured parsing', () => {
  test('parses recoverable profile details and actions', () => {
    const result = parseLauncherError({
      code: 'ERR_PROFILE_CORRUPT',
      message: 'Profile corrupted: bad data',
      details: {
        recoverable_issue: {
          kind: 'CorruptProfile',
          profile_path: '/some/path.json',
          reasons: ['Truncated JSON', 'Missing field "id"'],
        },
        suggested_actions: ['reinstall_loader', 'use_delegated_launch', 'dismiss'],
      },
      suggested_action: null,
    });

    expect(result.code).toBe('ERR_PROFILE_CORRUPT');
    expect(result.recoverableIssue?.kind).toBe('CorruptProfile');
    expect(result.recoverableIssue?.reasons).toEqual(['Truncated JSON', 'Missing field "id"']);
    expect(result.availableActions).toEqual(['reinstall_loader', 'use_delegated_launch', 'dismiss']);
  });

  test('generic errors have no recovery actions', () => {
    const result = parseLauncherError('Something went wrong');
    expect(result.code).toBe('ERR_UNKNOWN');
    expect(result.message).toBe('Something went wrong');
    expect(result.recoverableIssue).toBeNull();
    expect(result.availableActions).toEqual([]);
  });
});

test.describe('Profile recovery warning panel UI', () => {
  test('shows recovery actions and Dismiss does not relaunch', async ({ page }) => {
    await page.addInitScript(() => {
      const callbacks = new Map<number, (...args: unknown[]) => void>();
      let callbackId = 0;
      const eventListeners = new Map<string, number>();
      const invokeCalls: string[] = [];
      const row = {
        instance_id: 'recovery-test',
        name: 'Recovery Test',
        loader: 'forge',
        loader_version: '47.4.21',
        minecraft_version: '1.20.1',
        is_locked: false,
        last_launched_at: null,
      };

      const internals = {
        transformCallback(callback: (...args: unknown[]) => void) {
          const id = ++callbackId;
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback(id: number) { callbacks.delete(id); },
        invoke(command: string, args: Record<string, unknown> = {}) {
          invokeCalls.push(command);
          if (command === 'get_setting') {
            if (args.key === 'onboarding_complete') return Promise.resolve(true);
            if (args.key === 'launch_mode') return Promise.resolve('direct');
            return Promise.resolve(false);
          }
          if (command === 'get_windows_accent_color') return Promise.resolve(null);
          if (command === 'plugin:event|listen') {
            eventListeners.set(args.event as string, args.handler as number);
            return Promise.resolve(1);
          }
          if (command === 'plugin:event|unlisten' || command.startsWith('plugin:event|')) {
            return Promise.resolve(1);
          }
          if (command === 'query_launch_state') return Promise.resolve(null);
          if (command === 'list_instances') return Promise.resolve([row]);
          if (command === 'check_instance_crash') return Promise.resolve(null);
          if (command === 'check_instance_health') {
            return Promise.resolve({ score: 'green', blockers: [], warnings: [] });
          }
          if (command === 'launch_instance_direct') {
            return Promise.reject({
              code: 'ERR_PROFILE_CORRUPT',
              message: 'Profile corrupted: SHA-256 mismatch in profile JSON',
              details: {
                recoverable_issue: {
                  kind: 'CorruptProfile',
                  profile_path: null,
                  reasons: ['SHA-256 mismatch in profile JSON'],
                },
                suggested_actions: ['reinstall_loader', 'use_delegated_launch', 'dismiss'],
              },
              suggested_action: null,
            });
          }
          if (command === 'launch_instance') return Promise.resolve(null);
          if (command === 'repair_instance_loader') {
            return Promise.resolve({
              tuple: { loader: 'forge', minecraft_version: '1.20.1', loader_version: '47.4.21' },
              profile_id: 'forge-1.20.1-47.4.21',
              cache_hit: false,
              profile_stable_hash: 'abc',
              receipt_schema_version: 2,
              installer_exit_status: 0,
            });
          }
          return Promise.resolve(null);
        },
      };

      Object.assign(window as unknown as Record<string, unknown>, {
        __TAURI_INTERNALS__: internals,
        __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
        __tauriEventListeners: eventListeners,
        __callbacks: callbacks,
        __invokeCalls: invokeCalls,
      });
    });

    await page.goto('/');
    await page.getByRole('button', { name: 'My Instances' }).click();
    await page.getByRole('button', { name: 'Launch' }).click();

    const warningPanel = page.getByTestId('recoverable-profile-warning');
    await expect(warningPanel).toBeVisible();
    await expect(page.getByRole('button', { name: /Reinstall loader/ })).toBeVisible();
    await expect(page.getByRole('button', { name: /Use delegated launch/ })).toBeVisible();

    const dismiss = page.getByRole('button', { name: /Dismiss/ });
    await expect(dismiss).toBeVisible();
    await dismiss.click();
    await expect(warningPanel).toHaveCount(0);

    const launchAttempts = await page.evaluate(() => {
      const calls = (window as unknown as { __invokeCalls: string[] }).__invokeCalls;
      return calls.filter((call) => call === 'launch_instance_direct' || call === 'launch_instance').length;
    });
    expect(launchAttempts).toBe(1);
  });
});
