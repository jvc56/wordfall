import { test, expect, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PORT, PROJECT } from '../stack';
import { createCascade, newUser, press } from './support';

// PLAN.md § End-to-end tests → Updating the app: a second build deployed under
// an open card, "a new version is ready", MIN_APP_VERSION raised past the old
// build ("reload to keep syncing", which a plain reload does not clear and its
// own reload does), and a second tab that keeps its revealed card.

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');

function stackUp(env: string[]) {
	execFileSync('python3', [path.join(root, 'scripts/stack.py'), 'up', '--project', PROJECT, '--port', String(PORT), '--no-build', ...env.flatMap((e) => ['--env', e])], { stdio: 'inherit' });
}

async function build(page: Page) {
	return Number(await page.evaluate(() => document.documentElement.dataset.build));
}

test('updating the app', async ({ context }) => {
	test.setTimeout(600_000);
	const a = await context.newPage();
	await newUser(a, 'upd');
	const player = await createCascade(a, { name: 'Update' });
	await a.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
	await a.reload();
	await expect(a.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	const old = await build(a);
	// A second tab on the old build, with a card revealed.
	const b = await context.newPage();
	await b.goto(player);
	await expect(b.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	await press(b, 'Space');
	await expect(b.getByText('✓ Correct')).toBeVisible();
	try {
		// Deploy a second build.
		execFileSync('docker', ['compose', '-p', PROJECT, 'up', '-d', '--build', '--no-deps', 'frontend'], {
			cwd: root,
			env: { ...process.env, WORDFALL_APP_BUILD: String(old + 1), WORDFALL_PORT: String(PORT) },
			stdio: 'inherit'
		});
		// The new container takes a moment to answer: look for the new worker until it does.
		await expect
			.poll(() => a.evaluate(async () => (await navigator.serviceWorker.getRegistration())?.update().then(() => true, () => false)), { timeout: 60_000 })
			.toBe(true);
		await expect(a.getByText('A new version is ready.')).toBeVisible({ timeout: 60_000 });
		// The card is undisturbed.
		await expect(a.getByText('1 / 18')).toBeVisible();
		expect(await build(a)).toBe(old);
		// MIN_APP_VERSION past the old build: its next sync is answered 426.
		stackUp([`MIN_APP_VERSION=${old + 1}`]);
		await press(a, 'Space');
		await press(a, 'Space');
		await expect(a.getByRole('button', { name: 'Reload to keep syncing' })).toBeVisible({ timeout: 60_000 });
		// A plain reload does not clear it: the old worker still answers.
		await a.reload();
		await expect(a.getByRole('button', { name: 'Reload to keep syncing' })).toBeVisible({ timeout: 60_000 });
		expect(await build(a)).toBe(old);
		// Its own reload does: the new build, and the next sync succeeds.
		await a.getByRole('button', { name: 'Reload to keep syncing' }).click();
		await expect.poll(() => build(a), { timeout: 60_000 }).toBe(old + 1);
		await expect(a.getByText('Synced')).toBeVisible({ timeout: 60_000 });
		// The other tab did not reload: it keeps its revealed card, says the app was
		// updated in another tab, and finishes that card from the old build.
		await expect(b.getByText('Wordfall was updated in another tab — reload to continue')).toBeVisible({ timeout: 30_000 });
		await expect(b.getByText('✓ Correct')).toBeVisible();
		expect(await build(b)).toBe(old);
		await press(b, 'Space');
		await expect(b.getByText('2 / 18')).toBeVisible();
	} finally {
		stackUp([]);
	}
});
