import { test, expect, chromium, type Page } from '@playwright/test';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { apiCascade, createCascade, idb, login, newUser, playLevel, playerPref, press } from './support';

// PLAN.md § End-to-end tests: the shorter journeys.

async function ready(page: Page) {
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
}

test('leave decimals at 3 render a leave answer to three places', async ({ page }) => {
	await newUser(page, 'leave');
	const player = await apiCascade(page, {
		name: 'Leaves',
		quiz_type: 'leave_value',
		filters: { op: 'and', children: [{ type: 'leave_value', negated: false, min: 30.0, max: null }] }
	});
	await page.goto(player);
	await expect(page.getByText(/^1 \/ \d+$/)).toBeVisible({ timeout: 30_000 });
	await playerPref(page, 'Leave value decimal places', '3');
	await press(page, 'Space');
	await expect(page.getByText(/^\+\d+\.\d{3}$/)).toBeVisible();
});

test('hooks read Zyzzyva-style in lower case, and a Catalan multi-character hook reads NY', async ({ page }) => {
	await newUser(page, 'hooks');
	const port = await apiCascade(page, { name: 'Port', filters: { op: 'and', children: [{ type: 'anagram_match', negated: false, pattern: 'O P R T' }] } });
	await page.goto(port);
	await expect(page.getByText('1 / 1')).toBeVisible({ timeout: 30_000 });
	await playerPref(page, 'Show hooks with anagrams', true);
	await playerPref(page, 'Show definitions with anagrams', true);
	await press(page, 'Space');
	await expect(page.getByText('a harbour')).toBeVisible({ timeout: 30_000 });
	const li = page.getByRole('listitem').filter({ hasText: /PORT/ });
	await expect(li).toContainText('s');
	await expect(li).not.toContainText('SPORT');
	const catalan = await apiCascade(page, {
		name: 'Catalan',
		lexicon: 'CA-FIX',
		filters: { op: 'and', children: [{ type: 'length', negated: false, min: 1, max: 1 }] }
	});
	await page.goto(catalan);
	await expect(page.getByText('1 / 1')).toBeVisible({ timeout: 30_000 });
	await press(page, 'Space');
	const a = page.getByRole('listitem').filter({ hasText: /NY/ });
	await expect(a).toBeVisible({ timeout: 30_000 });
	await expect(page.getByRole('listitem').filter({ hasText: /ny/ })).toHaveCount(0);
});

test('a cascade trashed offline is listed in the Trash with "purges 30 days after this syncs"', async ({ page, context }) => {
	await newUser(page, 'trashoff');
	await createCascade(page, { name: 'Offline trash' });
	await page.goto('/cascades');
	await expect(page.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await ready(page);
	await context.setOffline(true);
	await page.getByRole('button', { name: 'Move to Trash' }).click();
	await expect(page.getByText('Offline trash')).toHaveCount(0);
	await page.goto('/trash');
	await expect(page.getByText('Offline trash (cascade in the Trash)')).toBeVisible();
	await expect(page.getByText('purges 30 days after this syncs')).toBeVisible();
	await context.setOffline(false);
	await page.goto('/trash');
	await expect(page.getByText(/^purges \d/).first()).toBeVisible({ timeout: 30_000 });
});

test('Delete forever is on a trashed cascade, not on the quizzes listed under it', async ({ page }) => {
	test.setTimeout(180_000);
	await newUser(page, 'delfor');
	await createCascade(page, { name: 'Delete forever' });
	expect(await playLevel(page, (i) => i < 2)).toMatch(/^Level 1/);
	expect(await playLevel(page, () => false)).toMatch(/^Level 2 cleared/);
	await page.goto('/cascades');
	await page.getByRole('button', { name: 'Move to Trash' }).click();
	await expect(page.getByRole('button', { name: 'Move to Trash' })).toHaveCount(0);
	await page.goto('/trash');
	await page.getByRole('button', { name: /Delete forever \(cascade in the Trash\)/ }).click();
	const cascadeEntry = page.getByRole('listitem').filter({ hasText: /^The whole cascade/ });
	const quizEntry = page.getByRole('listitem').filter({ hasText: /^Level 2/ });
	await expect(cascadeEntry.getByRole('button', { name: 'Delete forever' })).toBeVisible();
	await expect(quizEntry.getByRole('button', { name: 'Delete forever' })).toHaveCount(0);
	await expect(quizEntry.getByRole('button', { name: 'Restore' })).toBeVisible();
});

test('offline, restoring a quiz of a cascade never opened here downloads when online, without opening it', async ({ page, browser, baseURL }) => {
	test.setTimeout(180_000);
	const { username } = await newUser(page, 'restore');
	await createCascade(page, { name: 'Never opened' });
	expect(await playLevel(page, (i) => i < 2)).toMatch(/^Level 1/);
	expect(await playLevel(page, () => false)).toMatch(/^Level 2 cleared/);
	await page.goto('/cascades');
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 30_000 });
	const other = await browser.newContext({ baseURL });
	const p2 = await other.newPage();
	await login(p2, username);
	await expect(p2.getByText('Not downloaded on this device, open to download')).toBeVisible({ timeout: 30_000 });
	await ready(p2);
	await other.setOffline(true);
	await p2.goto('/trash');
	// A group of one entry is rendered expanded.
	await expect(p2.getByText('Downloads when online.')).toBeVisible();
	await p2.getByRole('button', { name: 'Restore' }).first().click();
	await expect(p2.getByText('The Trash is empty.')).toBeVisible();
	await other.setOffline(false);
	await p2.goto('/cascades');
	await expect(p2.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await other.close();
});

test('keys before answers: three cascades, offline before their answers arrive, every question playable', async ({ page, context }) => {
	test.setTimeout(180_000);
	await newUser(page, 'keys');
	const players: string[] = [];
	for (const [n, len] of [['K1', 5], ['K2', 6], ['K3', 7]] as const) players.push(await createCascade(page, { name: n, min: len, max: len }));
	await ready(page);
	// Every question has arrived — the keys, first — while the answers may still be on their way.
	await expect
		.poll(() => idb<number>(page, `return (await req(db.transaction('questions').objectStore('questions').getAll())).length;`), { timeout: 60_000 })
		.toBeGreaterThanOrEqual(3);
	await expect.poll(async () => {
		let missing = 0;
		for (const p of players) {
			const id = p.split('/').pop();
			missing += await idb<number>(page, `const c = await req(db.transaction('cascades').objectStore('cascades').get(arg)); const k = await req(db.transaction('questions').objectStore('questions').index('cascade_id').count(arg)); return c.question_count - k;`, id);
		}
		return missing;
	}, { timeout: 60_000 }).toBe(0);
	await context.setOffline(true);
	for (const p of players) {
		await page.goto(p);
		await expect(page.getByText(/^1 \/ \d+$/)).toBeVisible({ timeout: 30_000 });
		await expect(page.getByText('This question needs a connection.')).toHaveCount(0);
		await press(page, 'Space');
		await expect(page.getByText(/✓ Correct/)).toBeVisible();
	}
	await context.setOffline(false);
});

test('a browser restarted with a live session syncs rather than failing CSRF', async ({ baseURL }) => {
	test.setTimeout(120_000);
	const dir = mkdtempSync(path.join(tmpdir(), 'wordfall-restart-'));
	try {
		let ctx = await chromium.launchPersistentContext(dir, { baseURL });
		let page = ctx.pages()[0] ?? (await ctx.newPage());
		await newUser(page, 'restart');
		const player = await createCascade(page, { name: 'Restart' });
		await press(page, 'Space');
		await press(page, 'Space');
		await ctx.close();
		ctx = await chromium.launchPersistentContext(dir, { baseURL });
		page = ctx.pages()[0] ?? (await ctx.newPage());
		await page.goto(player);
		await press(page, 'Space');
		await press(page, 'Space');
		await page.goto('/cascades');
		await expect(page.getByText('Synced')).toBeVisible({ timeout: 30_000 });
		await ctx.close();
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
});
