// The signed-in account's database connection, one per tab (PLAN.md § On the
// device → IndexedDB stores: no per-user database but the signed-in one is
// ever opened). A `versionchange` from a newer build in another tab closes it
// and raises the "updated in another tab" notice.
import { openUserDb, type UserDb } from './db';
import { initMeta } from './meta';

let current: { userId: string; db: Promise<UserDb> } | null = null;
const listeners = new Set<() => void>();

/** Called when another tab's newer build needs the database. */
export function onUpdatedElsewhere(fn: () => void): () => void {
	listeners.add(fn);
	return () => listeners.delete(fn);
}

/** The signed-in account's database, opened once and kept for the tab's life. */
export function userDb(userId: string, username = ''): Promise<UserDb> {
	if (current?.userId === userId) return current.db;
	closeUserDb();
	const db = openUserDb(userId, () => {
		if (current?.userId === userId) current = null;
		for (const fn of listeners) fn();
	}).then(async (db) => {
		await initMeta(db, userId, username);
		return db;
	});
	current = { userId, db };
	db.catch(() => {
		if (current?.db === db) current = null;
	});
	return db;
}

/** Closes the connection, before a logout or removing the account's data. */
export function closeUserDb() {
	const c = current;
	current = null;
	c?.db.then((db) => db.close()).catch(() => undefined);
}
