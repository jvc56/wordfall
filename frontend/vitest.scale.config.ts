// `make test-scale`, the device's half (PLAN.md § Scale tests): the
// *.scale.ts files in a real Chromium, whose IndexedDB is the one the budgets
// are about (fake-indexeddb in Node writes about a thousand rows a second,
// which measures fake-indexeddb). One file at a time, with no per-test timeout
// short of the budgets the tests assert themselves.
import { playwright } from '@vitest/browser-playwright';
import { defineConfig, mergeConfig } from 'vitest/config';
import base from './vite.config';

export default mergeConfig(
	base,
	defineConfig({
		test: {
			include: ['src/**/*.scale.ts'],
			fileParallelism: false,
			testTimeout: 4 * 60 * 60 * 1000,
			hookTimeout: 60 * 60 * 1000,
			browser: {
				enabled: true,
				headless: true,
				// A persistent profile: a fresh context is incognito, whose small
				// in-memory storage quota a 300,000-question cascade exceeds.
				provider: playwright({
					persistentContext: 'node_modules/.cache/wordfall-scale-profile',
					launchOptions: { args: ['--js-flags=--max-old-space-size=8192'] }
				}),
				instances: [{ browser: 'chromium' }]
			}
		}
	})
);
