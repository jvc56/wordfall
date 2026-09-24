import { test, expect, type Page } from '@playwright/test';
import { createCascade, login, newUser, press } from './support';

// PLAN.md § End-to-end tests → Logging out; § Authentication while offline.

async function logOut(page: Page, removeData = false) {
	await page.goto('/account');
	if (removeData) await page.getByLabel("Remove this account's data from this device").first().check();
	await page.getByRole('button', { name: 'Log out' }).click();
	await expect(page).toHaveURL(/\/login$/);
}

async function ready(page: Page) {
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
}

test('logging out, online and offline, and a second account on the same browser', async ({ page, context }) => {
	test.setTimeout(240_000);
	const alice = await newUser(page, 'alice');
	await createCascade(page, { name: 'Alice cascade' });
	await page.goto('/cascades');
	await expect(page.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await ready(page);

	// Nothing unsent: the login page, and it stays there through an offline reload.
	await logOut(page);
	await context.setOffline(true);
	await page.reload();
	await expect(page).toHaveURL(/\/login$/);
	await expect(page.getByText('Alice cascade')).toHaveCount(0);
	await context.setOffline(false);
	// Logging back in finds the cascade still local, with no download.
	const downloads: string[] = [];
	page.on('request', (r) => {
		if (/\/cards\?/.test(r.url())) downloads.push(r.url());
	});
	await login(page, alice.username);
	await expect(page.getByText('Alice cascade')).toBeVisible();
	await expect(page.getByText('Available offline')).toBeVisible();
	await page.waitForTimeout(2000);
	expect(downloads).toEqual([]);

	// Offline: Log out still lands on the login page; on reconnecting the queued
	// logout goes through and the session cookie is gone.
	await context.setOffline(true);
	await logOut(page);
	const loggedOut = page.waitForResponse((r) => r.url().endsWith('/api/auth/logout'));
	await context.setOffline(false);
	expect((await loggedOut).status()).toBeLessThan(300);
	expect((await context.cookies()).find((c) => c.name === 'wordfall_session')).toBeUndefined();

	// Unsent work: the dialog names the count.
	await login(page, alice.username);
	const player = new URL((await page.getByRole('link', { name: 'Alice cascade' }).getAttribute('href'))!, page.url()).pathname;
	await context.setOffline(true);
	await page.goto(player);
	await expect(page.getByText(/^\d+ \/ \d+$/).first()).toBeVisible({ timeout: 20_000 });
	await press(page, 'Space');
	await press(page, 'Space');
	// The grade and the cursor move are written once the next card shows.
	await expect(page.getByText(/^2 \/ \d+$/).first()).toBeVisible();
	await page.goto('/account');
	await expect(page.getByText('2 changes haven’t synced yet. They will sync the next time you log in on this device.')).toBeVisible();
	await context.setOffline(false);

	// A second account sees none of the first's cascades, and lists both accounts.
	await context.setOffline(true);
	await logOut(page);
	// The queued logout cannot get through yet …
	await page.route('**/api/auth/logout', (r) => r.abort());
	await context.setOffline(false);
	const bob = await newUser(page, 'bob');
	// … and a login clears it: no logout is sent while Bob is signed in.
	await page.unroute('**/api/auth/logout');
	const sent: string[] = [];
	page.on('request', (r) => {
		if (r.url().endsWith('/api/auth/logout')) sent.push(r.url());
	});
	await expect(page.getByText('Alice cascade')).toHaveCount(0);
	await page.goto('/account');
	await expect(page.getByText(/^alice\w+: rows and keys/)).toBeVisible();
	await expect(page.getByText(new RegExp(`^${bob.username}: rows and keys`))).toBeVisible();
	await page.waitForTimeout(2000);
	expect(sent).toEqual([]);
	// Bob's sync still succeeds.
	await page.goto('/cascades');
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 30_000 });
	// Removing Alice's data leaves Bob's.
	await page.goto('/account');
	await page.getByRole('listitem').filter({ hasText: /^alice/ }).getByRole('button', { name: "Remove this account's data from this device" }).click();
	await expect(page.getByText(/^alice\w+: rows and keys/)).toHaveCount(0);
	await expect(page.getByText(new RegExp(`^${bob.username}: rows and keys`))).toBeVisible();
	// Removing his own clears the pointer and lands on the login page.
	await page.getByRole('listitem').filter({ hasText: new RegExp(`^${bob.username}`) }).getByRole('button', { name: "Remove this account's data from this device" }).click();
	await expect(page).toHaveURL(/\/login$/);
	// Alice's unsent grades went with her data; logging in again she starts from the server.
	await login(page, alice.username);
	await expect(page.getByText('Alice cascade')).toBeVisible({ timeout: 30_000 });
});
