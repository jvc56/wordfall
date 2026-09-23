import { defineConfig, devices } from '@playwright/test';

// PLAN.md § End-to-end tests. globalSetup brings a Wordfall up through the same
// scripts/stack.py entry points ./scripts/dev.py uses, as project wordfall-e2e
// on a port from the environment, unless BASE_URL names an existing instance.
const port = Number(process.env.E2E_PORT ?? 5180);

export default defineConfig({
	testDir: './tests',
	globalSetup: './global-setup.ts',
	globalTeardown: './global-teardown.ts',
	fullyParallel: false,
	workers: 1,
	retries: 0,
	timeout: 120_000,
	reporter: [['list']],
	use: {
		baseURL: process.env.BASE_URL ?? `http://localhost:${port}`,
		trace: 'retain-on-failure',
		serviceWorkers: 'allow'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }]
});
