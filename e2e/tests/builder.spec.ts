import { test, expect } from '@playwright/test';

// A smoke run of the builder against the seeded fixture catalog (PLAN.md §
// Creating a cascade): preview as filters change, then Create Cascade.
test('the builder previews and creates a cascade', async ({ page }) => {
	await page.goto('/login');
	await page.getByLabel('Username').fill('dev');
	await page.getByLabel('Password').fill('correct-tile-rack-bingo');
	await page.getByRole('button', { name: 'Log in' }).click();
	await expect(page).toHaveURL(/\/cascades$/);
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill('7');
	await page.getByLabel('Maximum').first().fill('7');
	await expect(page.getByText('18 questions')).toBeVisible({ timeout: 15_000 });
	await expect(page.getByLabel('Cascade name')).toHaveValue('EN-FIX · Length 7–7');
	await page.getByRole('button', { name: 'Create Cascade' }).click();
	await expect(page).toHaveURL(/\/cascades\/[0-9a-f-]{36}$/);
	const id = page.url().split('/').pop()!;
	// The new cascade is in this device's base at once, with its Source quiz's
	// 18 rows, and the sync engine has synced against the real server since
	// (PLAN.md § Downloads, § The sync cycle).
	await expect
		.poll(
			() =>
				page.evaluate(async (cascadeId) => {
					const open = (name: string) =>
						new Promise<IDBDatabase>((res, rej) => {
							const r = indexedDB.open(name);
							r.onsuccess = () => res(r.result);
							r.onerror = () => rej(r.error);
						});
					const get = <T>(db: IDBDatabase, store: string, key: IDBValidKey) =>
						new Promise<T>((res) => {
							const r = db.transaction(store).objectStore(store).get(key);
							r.onsuccess = () => res(r.result as T);
						});
					const count = (db: IDBDatabase, store: string, index: string, key: IDBValidKey) =>
						new Promise<number>((res) => {
							const r = db.transaction(store).objectStore(store).index(index).count(key);
							r.onsuccess = () => res(r.result);
						});
					const unscoped = await open('wordfall');
					const pointer = await get<{ user_id: string }>(unscoped, 'pointer', 'signed_in');
					const db = await open(`wordfall-user-${pointer.user_id}`);
					const sync = await get<{ cursor: number | null }>(db, 'meta', 'sync');
					const rows = await count(db, 'quiz_questions', 'cascade_id', cascadeId);
					return { synced: sync?.cursor != null, rows };
				}, id),
			{ timeout: 15_000 }
		)
		.toEqual({ synced: true, rows: 18 });
});
