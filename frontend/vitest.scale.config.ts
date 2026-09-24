// `make test-scale`, the device's half (PLAN.md § Scale tests): the
// *.scale.ts files, each in its own process with room for 300,000-question
// cascades in fake-indexeddb, one at a time, with no per-test timeout short
// of the budgets the tests assert themselves.
import { defineConfig, mergeConfig } from 'vitest/config';
import base from './vite.config';

export default mergeConfig(
	base,
	defineConfig({
		test: {
			include: ['src/**/*.scale.ts'],
			pool: 'forks',
			fileParallelism: false,
			execArgv: ['--max-old-space-size=12288'],
			testTimeout: 4 * 60 * 60 * 1000,
			hookTimeout: 60 * 60 * 1000
		}
	})
);
