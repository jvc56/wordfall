import { test, expect, type Page } from '@playwright/test';
import { createCascade, idb, login, newUser, outboxCount, PASSWORD, press } from './support';

// PLAN.md § End-to-end tests: switching accounts in another tab, and deleting the account.

async function logOut(page: Page) {
	await page.goto('/account');
	await page.getByRole('button', { name: 'Log out' }).click();
	await expect(page).toHaveURL(/\/login$/);
}

test('switching accounts in another tab', async ({ context }) => {
	test.setTimeout(240_000);
	const a = await context.newPage();
	const alice = await newUser(a, 'swa');
	const player = await createCascade(a, { name: 'Alice twenty', min: 6, max: 8 });
	await expect(a.getByText(/^1 \/ \d+$/).first()).toBeVisible({ timeout: 30_000 });
	await a.goto('/cascades');
	await expect(a.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await a.goto(player);
	// This tab alone is offline.
	await a.route('**/api/**', (r) => r.abort('internetdisconnected'));
	for (let i = 0; i < 20; i++) {
		await press(a, 'Space');
		await press(a, 'Space');
	}
	expect(await outboxCount(a, 'grade')).toBe(20);
	const aliceId = await idb<string>(a, `return (await req(db.transaction('meta').objectStore('meta').get('identity'))).user_id;`);
	// A second tab: log out, log in as Bob.
	const b = await context.newPage();
	await b.goto('/cascades');
	await logOut(b);
	await newUser(b, 'swb');
	await createCascade(b, { name: 'Bob cascade' });
	await expect(a.getByText('Signed out in another tab')).toBeVisible({ timeout: 10_000 });
	// Reconnect: nothing from the first tab is applied as Bob.
	await a.unroute('**/api/**');
	await a.waitForTimeout(3000);
	const inAlice = await a.evaluate(async (uid) => {
		const db = await new Promise<IDBDatabase>((res) => {
			const r = indexedDB.open(`wordfall-user-${uid}`);
			r.onsuccess = () => res(r.result);
		});
		const get = <T>(store: string) =>
			new Promise<T[]>((res) => {
				const r = db.transaction(store).objectStore(store).getAll();
				r.onsuccess = () => res(r.result as T[]);
			});
		const grades = (await get<{ op: { type: string } }>('outbox')).filter((e) => e.op.type === 'grade').length;
		const bobs = (await get<{ name: string }>('cascades')).filter((c) => c.name === 'Bob cascade').length;
		db.close();
		return { grades, bobs };
	}, aliceId);
	// Alice's twenty grades are still in her outbox, and none of Bob's cascades is in her database.
	expect(inAlice).toEqual({ grades: 20, bobs: 0 });
	// Logging in as Alice again pushes the twenty grades.
	await b.close();
	await login(a, alice.username);
	await a.goto('/cascades');
	await expect(a.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	expect(await outboxCount(a)).toBe(0);
	// Two tabs as Alice: one logs out and back in; the other passes through
	// "signed out in another tab" and resumes without a reload.
	const c2 = await context.newPage();
	await c2.goto('/cascades');
	await expect(c2.getByText('Synced')).toBeVisible({ timeout: 30_000 });
	await logOut(a);
	await expect(c2.getByText('Signed out in another tab')).toBeVisible({ timeout: 10_000 });
	await login(a, alice.username);
	await expect(c2.getByText('Signed out in another tab')).toHaveCount(0, { timeout: 10_000 });
	await c2.goto(player);
	await press(c2, 'Space');
	await press(c2, 'Space');
	await c2.goto('/cascades');
	await expect(c2.getByText('Synced')).toBeVisible({ timeout: 30_000 });
	void PASSWORD;
});

test('deleting the account', async ({ browser, baseURL }) => {
	test.setTimeout(180_000);
	const ctx = await browser.newContext({ baseURL });
	const page = await ctx.newPage();
	// A second account's data on the same browser.
	const other = await newUser(page, 'keep');
	await createCascade(page, { name: 'Kept' });
	await logOut(page);
	const doomed = await newUser(page, 'doom');
	await createCascade(page, { name: 'Doomed' });
	// A second browser still signed in as the doomed account.
	const ctx2 = await browser.newContext({ baseURL });
	const p2 = await ctx2.newPage();
	await login(p2, doomed.username);
	await expect(p2.getByText('Doomed')).toBeVisible({ timeout: 30_000 });
	// Delete it.
	await page.goto('/account');
	await page.getByLabel('Password').last().fill(PASSWORD);
	await page.getByRole('button', { name: 'Delete account' }).click();
	await expect(page).toHaveURL(/\/$/, { timeout: 30_000 });
	const dbs = await page.evaluate(async () => {
		const open = (name: string) =>
			new Promise<IDBDatabase>((res) => {
				const r = indexedDB.open(name);
				r.onsuccess = () => res(r.result);
			});
		const u = await open('wordfall');
		const rows = await new Promise<{ username: string }[]>((res) => {
			const r = u.transaction('accounts').objectStore('accounts').getAll();
			r.onsuccess = () => res(r.result);
		});
		u.close();
		return rows.map((r) => r.username);
	});
	expect(dbs).toEqual([other.username]);
	await page.reload();
	await expect(page.getByText('Doomed')).toHaveCount(0);
	// The other account's storage line lists only itself.
	await login(page, other.username);
	await page.goto('/account');
	await expect(page.getByText(new RegExp(`^${other.username}: rows and keys`))).toBeVisible();
	await expect(page.getByText(new RegExp(`^${doomed.username}: rows and keys`))).toHaveCount(0);
	// The second browser: its next sync answers 401; Log in to sync, cascades still readable.
	await p2.reload();
	await expect(p2.getByText('Log in to sync')).toBeVisible({ timeout: 60_000 });
	await expect(p2.getByText('Doomed')).toBeVisible();
	await p2.goto('/account');
	await expect(p2.getByRole('button', { name: "Remove this account's data from this device" }).first()).toBeVisible();
	await ctx.close();
	await ctx2.close();
});
