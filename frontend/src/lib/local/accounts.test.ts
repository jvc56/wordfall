// PLAN.md § Unit tests → Frontend: "They also cover the unscoped database".
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { openDB } from 'idb';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
	getAccount,
	recordLogin,
	recordLogout,
	recordTotals,
	removeAccountData,
	retryQueuedLogout,
	signedIn,
	storageLine,
	userDbName
} from './accounts';

beforeEach(() => {
	globalThis.indexedDB = new IDBFactory();
	// Safari and Firefox lack indexedDB.databases(); the whole path runs without it.
	(globalThis.indexedDB as unknown as Record<string, unknown>).databases = undefined;
});

async function userDbExists(userId: string): Promise<boolean> {
	let created = false;
	const db = await openDB(userDbName(userId), undefined, {
		upgrade() {
			created = true;
		}
	});
	db.close();
	return !created;
}

async function makeUserDb(userId: string) {
	const db = await openDB(userDbName(userId), 1, {
		upgrade(d) {
			d.createObjectStore('meta');
		}
	});
	db.close();
}

describe('unscoped database', () => {
	it('two accounts signing in leave two rows and one pointer', async () => {
		await recordLogin('a', 'alice');
		await recordLogin('b', 'bob');
		expect((await storageLine()).map((r) => r.user_id).sort()).toEqual(['a', 'b']);
		expect(await signedIn()).toBe('b');
	});

	it('a logout clears the pointer and no row', async () => {
		await recordLogin('a', 'alice');
		await recordLogout('a');
		expect(await signedIn()).toBeNull();
		expect((await getAccount('a'))?.logout_pending).toBe(true);
		expect(await storageLine()).toHaveLength(1);
	});

	it('removing data with no pending logout deletes its row and database, the other survives', async () => {
		await recordLogin('a', 'alice');
		await makeUserDb('a');
		await recordLogin('b', 'bob');
		await makeUserDb('b');
		await removeAccountData('a');
		expect(await getAccount('a')).toBeUndefined();
		expect(await userDbExists('a')).toBe(false);
		expect(await getAccount('b')).toBeDefined();
		expect(await userDbExists('b')).toBe(true);
		expect(await signedIn()).toBe('b');
	});

	it('removing the signed-in account clears the pointer', async () => {
		await recordLogin('a', 'alice');
		await removeAccountData('a');
		expect(await signedIn()).toBeNull();
	});

	it("storage figures for another account come from its row, with no open on its database", async () => {
		await recordLogin('a', 'alice');
		await recordTotals('a', { rows: 10, keys: 20, answer_bytes: 30 });
		await recordLogin('b', 'bob');
		const open = vi.spyOn(globalThis.indexedDB, 'open');
		const line = await storageLine();
		const opened = open.mock.calls.map((c) => c[0]);
		expect(opened.every((n) => n === 'wordfall')).toBe(true);
		expect(line.find((r) => r.user_id === 'a')).toMatchObject({ rows: 10, keys: 20, answer_bytes: 30 });
		open.mockRestore();
	});

	it('the queued logout survives data removal as a bare row, absent from the storage line', async () => {
		await recordLogin('a', 'alice');
		await recordLogout('a');
		await removeAccountData('a');
		const row = await getAccount('a');
		expect(row).toMatchObject({ user_id: 'a', username: 'alice', logout_pending: true, emptied: true });
		expect(await storageLine()).toHaveLength(0);
		const sent = await retryQueuedLogout(async () => ({ status: 204 }));
		expect(sent).toBe(true);
		expect(await getAccount('a')).toBeUndefined();
	});

	it.each([
		[{ status: 204 }, false],
		[{ status: 401 }, false],
		[{ status: 403 }, false],
		[{ status: 429 }, true],
		[{ status: 503 }, true],
		[{ networkError: true as const }, true]
	])('the flag after %o stays set: %s', async (outcome, stays) => {
		await recordLogin('a', 'alice');
		await recordLogout('a');
		await retryQueuedLogout(async () => outcome);
		expect((await getAccount('a'))?.logout_pending).toBe(stays);
	});

	it('the retry is sent at a startup with no pointer and not while an account is signed in', async () => {
		await recordLogin('a', 'alice');
		await recordLogout('a');
		await recordLogin('b', 'bob'); // clears every pending flag
		expect((await getAccount('a'))?.logout_pending).toBe(false);
		await recordLogout('b');
		// Signed in as someone: never sent.
		await recordLogin('a', 'alice');
		const send = vi.fn(async () => ({ status: 204 }));
		expect(await retryQueuedLogout(send)).toBe(false);
		expect(send).not.toHaveBeenCalled();
		await recordLogout('a');
		expect(await retryQueuedLogout(send)).toBe(true);
		expect(send).toHaveBeenCalledTimes(1);
	});

	it("a second account's login clears every pending flag", async () => {
		await recordLogin('a', 'alice');
		await recordLogout('a');
		await removeAccountData('a'); // bare row with the flag
		await recordLogin('b', 'bob');
		expect(await getAccount('a')).toBeUndefined();
		const send = vi.fn(async () => ({ status: 204 }));
		await recordLogout('b');
		await recordLogin('b', 'bob');
		await recordLogout('b');
		// Only b's own flag is pending now.
		await retryQueuedLogout(send);
		expect(send).toHaveBeenCalledTimes(1);
	});
});
