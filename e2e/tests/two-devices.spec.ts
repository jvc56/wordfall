import { test, expect } from '@playwright/test';
import { createCascade, login, newUser, playLevel } from './support';

// PLAN.md § End-to-end tests → Two devices: "finish the same level offline in
// two browser contexts, reconnect both, and see the second device's notice and
// matching final state".
test('two devices finish the same level offline', async ({ browser, baseURL }) => {
	test.setTimeout(240_000);
	const a = await browser.newContext({ baseURL });
	const b = await browser.newContext({ baseURL });
	const pa = await a.newPage();
	const pb = await b.newPage();
	const { username } = await newUser(pa, 'two');
	const player = await createCascade(pa, { name: 'Two devices' });
	await login(pb, username);
	await pb.goto(player);
	await expect(pb.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	await expect(pa.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	for (const [ctx, page] of [[a, pa], [b, pb]] as const) {
		await page.evaluate(async () => {
			await navigator.serviceWorker.ready;
		});
		await ctx.setOffline(true);
	}
	await playLevel(pa, (i) => i < 2);
	await playLevel(pb, (i) => i < 3);
	// A reconnects first and wins; then B.
	await a.setOffline(false);
	await pa.goto('/cascades');
	await expect(pa.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	await b.setOffline(false);
	await pb.goto('/cascades');
	await expect(pb.getByText('Level 1 was finished on another device. 18 answers from this device weren’t kept.')).toBeVisible({ timeout: 60_000 });
	await expect(pb.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	// The same final state on both: Level 2 with A's two misses.
	for (const page of [pa, pb]) {
		await page.goto(player);
		await expect(page.getByText('Level 2 of 2')).toBeVisible({ timeout: 30_000 });
		await expect(page.getByText('1 / 2')).toBeVisible();
	}
	await a.close();
	await b.close();
});
