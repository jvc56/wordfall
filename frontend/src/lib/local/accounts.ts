// The unscoped database (PLAN.md § On the device → IndexedDB stores, and
// § Authentication while offline): an `accounts` table with one row per
// account that has data here, and one `signed_in` pointer naming the account
// signed in now. No per-user database but the signed-in one is ever opened,
// and nothing here calls indexedDB.databases(), which Safari and Firefox lack.
import { deleteDB, openDB, type DBSchema, type IDBPDatabase } from 'idb';

export const UNSCOPED_DB = 'wordfall';
export const POINTER_KEY = 'signed_in';
export const SIGNED_IN_CHANNEL = 'wordfall-signed-in';

export function userDbName(userId: string): string {
	return `wordfall-user-${userId}`;
}

export interface AccountRow {
	user_id: string;
	username: string;
	/** ISO time of the last sign-in here; null on an emptied row. */
	last_signed_in_at: string | null;
	/** Its rows, keys and answer bytes as its own drop pass and eviction last measured them. */
	rows: number;
	keys: number;
	answer_bytes: number;
	/** Set while a POST /api/auth/logout for it is still unacknowledged. */
	logout_pending: boolean;
	/** Its data was removed; the row survives only to carry `logout_pending`. */
	emptied: boolean;
}

interface UnscopedSchema extends DBSchema {
	accounts: { key: string; value: AccountRow };
	pointer: { key: string; value: { user_id: string } };
}

/** Versioned migrations; index i upgrades from version i to i + 1. */
const MIGRATIONS: Array<(db: IDBPDatabase<UnscopedSchema>) => void> = [
	(db) => {
		db.createObjectStore('accounts', { keyPath: 'user_id' });
		db.createObjectStore('pointer');
	}
];
export const UNSCOPED_VERSION = MIGRATIONS.length;

export async function openUnscoped(): Promise<IDBPDatabase<UnscopedSchema>> {
	return openDB<UnscopedSchema>(UNSCOPED_DB, UNSCOPED_VERSION, {
		upgrade(db, oldVersion) {
			for (let v = oldVersion; v < UNSCOPED_VERSION; v++) MIGRATIONS[v](db);
		}
	});
}

function broadcast(userId: string | null) {
	if (typeof BroadcastChannel === 'undefined') return;
	const ch = new BroadcastChannel(SIGNED_IN_CHANNEL);
	ch.postMessage({ signed_in: userId });
	ch.close();
}

async function withDb<T>(fn: (db: IDBPDatabase<UnscopedSchema>) => Promise<T>): Promise<T> {
	const db = await openUnscoped();
	try {
		return await fn(db);
	} finally {
		db.close();
	}
}

/** The account signed in now, or null. Read before anything else at startup. */
export async function signedIn(): Promise<string | null> {
	return withDb(async (db) => (await db.get('pointer', POINTER_KEY))?.user_id ?? null);
}

export async function getAccount(userId: string): Promise<AccountRow | undefined> {
	return withDb((db) => db.get('accounts', userId));
}

/**
 * Login: insert or update the account's row, set the pointer, and clear the
 * unacknowledged-logout flag on every row, its own included, since the
 * response that signed this account in has already replaced the cookie those
 * queued logouts existed to clear.
 */
export async function recordLogin(userId: string, username: string, now = new Date()) {
	await withDb(async (db) => {
		const tx = db.transaction(['accounts', 'pointer'], 'readwrite');
		const accounts = tx.objectStore('accounts');
		for (const row of await accounts.getAll()) {
			if (row.logout_pending && row.user_id !== userId) {
				if (row.emptied) await accounts.delete(row.user_id);
				else await accounts.put({ ...row, logout_pending: false });
			}
		}
		const existing = await accounts.get(userId);
		await accounts.put({
			user_id: userId,
			username,
			last_signed_in_at: now.toISOString(),
			rows: existing && !existing.emptied ? existing.rows : 0,
			keys: existing && !existing.emptied ? existing.keys : 0,
			answer_bytes: existing && !existing.emptied ? existing.answer_bytes : 0,
			logout_pending: false,
			emptied: false
		});
		await tx.objectStore('pointer').put({ user_id: userId }, POINTER_KEY);
		await tx.done;
	});
	broadcast(userId);
}

/**
 * Logout always completes locally: the pointer is cleared whether or not the
 * request succeeded, and the request is queued as the flag on the row.
 */
export async function recordLogout(userId: string) {
	await withDb(async (db) => {
		const tx = db.transaction(['accounts', 'pointer'], 'readwrite');
		const row = await tx.objectStore('accounts').get(userId);
		if (row) await tx.objectStore('accounts').put({ ...row, logout_pending: true });
		await tx.objectStore('pointer').delete(POINTER_KEY);
		await tx.done;
	});
	broadcast(null);
}

/** The totals the drop pass and eviction last measured, kept on the row. */
export async function recordTotals(
	userId: string,
	totals: { rows: number; keys: number; answer_bytes: number }
) {
	await withDb(async (db) => {
		const row = await db.get('accounts', userId);
		if (row && !row.emptied) await db.put('accounts', { ...row, ...totals });
	});
}

/**
 * Remove this account's data from this device: delete its per-user database
 * and empty its row, which is deleted too unless it still holds an
 * unacknowledged logout. Removing the signed-in account's data also clears
 * the pointer.
 */
export async function removeAccountData(userId: string) {
	await deleteDB(userDbName(userId));
	let clearedPointer = false;
	await withDb(async (db) => {
		const tx = db.transaction(['accounts', 'pointer'], 'readwrite');
		const accounts = tx.objectStore('accounts');
		const row = await accounts.get(userId);
		if (row?.logout_pending) {
			await accounts.put({
				user_id: row.user_id,
				username: row.username,
				last_signed_in_at: null,
				rows: 0,
				keys: 0,
				answer_bytes: 0,
				logout_pending: true,
				emptied: true
			});
		} else if (row) {
			await accounts.delete(userId);
		}
		const pointer = await tx.objectStore('pointer').get(POINTER_KEY);
		if (pointer?.user_id === userId) {
			await tx.objectStore('pointer').delete(POINTER_KEY);
			clearedPointer = true;
		}
		await tx.done;
	});
	if (clearedPointer) broadcast(null);
}

/** The Account page's storage line: every account with data here, from its row alone. */
export async function storageLine(): Promise<AccountRow[]> {
	return withDb(async (db) => (await db.getAll('accounts')).filter((r) => !r.emptied));
}

export type LogoutOutcome = { status: number } | { networkError: true };

/**
 * The queued logout: sent only while no account is signed in. The flag
 * clears on a 2xx or on any 4xx but 429 — whether the request could ever be
 * accepted, not which status came back — and stays for a 429, a 5xx or a
 * network error. One request clears the one session cookie, so its outcome
 * applies to every pending row. Returns true when a request was sent.
 */
export async function retryQueuedLogout(send: () => Promise<LogoutOutcome>): Promise<boolean> {
	if ((await signedIn()) !== null) return false;
	const pending = await withDb(async (db) =>
		(await db.getAll('accounts')).filter((r) => r.logout_pending)
	);
	if (pending.length === 0) return false;
	const outcome = await send();
	const done =
		'status' in outcome &&
		((outcome.status >= 200 && outcome.status < 300) ||
			(outcome.status >= 400 && outcome.status < 500 && outcome.status !== 429));
	if (!done) return true;
	await withDb(async (db) => {
		const tx = db.transaction('accounts', 'readwrite');
		for (const p of pending) {
			const row = await tx.store.get(p.user_id);
			if (!row?.logout_pending) continue;
			if (row.emptied) await tx.store.delete(row.user_id);
			else await tx.store.put({ ...row, logout_pending: false });
		}
		await tx.done;
	});
	return true;
}
