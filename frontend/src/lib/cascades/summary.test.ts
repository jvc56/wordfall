// PLAN.md § Unit tests → Frontend: the Cascades page showing Available
// offline, not the download label, for a kept cascade outside the window; a
// quiz cleared offline and a cascade trashed offline getting provisional
// entries that read "purges 30 days after this syncs", the trashed cascade
// still listed in the Trash while gone from the Cascades page, and the
// acknowledged rows replacing both; a set_quiz_options with a segment size
// above the cap /api/auth/me reported being refused on the device.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { describe, expect, it } from 'vitest';
import { applyLocally, APPLY_STORES } from '$lib/local/apply';
import { openUserDb, type UserDb } from '$lib/local/db';
import { initMeta, recordOpen } from '$lib/local/meta';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { DownloadManager, type DownloadApi } from '$lib/sync/downloads';
import { SyncEngine } from '$lib/sync/engine';
import { setKeepOffline } from '$lib/sync/keep';
import { FakeServer } from '$lib/sync/testing/fake-server';
import { cascadeSummaries, ladderText, optionsSummary, trashGroups } from './summary';

const C = 'c1000000-0000-4000-8000-000000000000';
const Q = 'a1000000-0000-4000-8000-000000000000';
const DAY = 86_400_000;

async function device(server: FakeServer, clock: () => Date) {
	globalThis.indexedDB = new IDBFactory();
	const db: UserDb = await openUserDb('u');
	await initMeta(db, 'u', 'u');
	const engine = new SyncEngine(db, {}, { transport: async (r) => server.sync(JSON.parse(JSON.stringify(r))), wait: async () => undefined, now: clock });
	const api: DownloadApi = {
		questions: async (_c, q, f, l) => server.questions(q, f, l)!,
		grades: async (_c, q, f, l) => server.grades(q, f, l)!,
		keys: async (c, f, l) => server.keys(c, f, l)!,
		cards: async (c, f, l, h, d) => server.cards(c, f, l, h, d)!,
		distribution: async (name) => ({ name, tiles: [] })
	};
	const dl = new DownloadManager(db, { api, sync: () => engine.sync(), wait: async () => undefined, now: clock });
	return { db, engine, dl };
}

async function grade(db: UserDb, quizId: string, marks: string) {
	const tx = db.transaction(APPLY_STORES) as unknown as RwTx;
	const q = (await view.quiz(tx, quizId))!;
	const rows = (await view.questionsOf(tx, quizId)).sort((a, b) => a.position! - b.position!);
	for (const [i, m] of [...marks].entries()) {
		await applyLocally(db, {
			type: 'grade', quiz_id: quizId, attempt: q.attempt, attempt_seed: q.shuffle_seed, question_idx: rows[i].question_idx,
			grade: m === 'C' ? 'correct' : 'missed'
		});
	}
	return q;
}

describe('the Cascades and Trash pages', () => {
	it('summarise options and the ladder', () => {
		expect(optionsSummary({ segment_size: 40, progression: 'drill', require_alphabetical: false })).toBe('segments of 40 · drill');
		expect(optionsSummary({ segment_size: 0, progression: 'ladder', require_alphabetical: false })).toBe('');
	});

	it('show Available offline, not the download label, for a kept cascade outside the window', async () => {
		const server = new FakeServer();
		server.create({ id: C, source_id: Q, count: 4 });
		let now = Date.now();
		const { db, engine, dl } = await device(server, () => new Date(now));
		await engine.sync();
		expect((await cascadeSummaries(db, true))[0].offline).toBe('not_downloaded');
		await setKeepOffline(db, C, true, dl, new Date(now));
		now += 30 * DAY;
		await dl.run();
		const [s] = await cascadeSummaries(db, true);
		expect(s.offline).toBe('available');
		expect(s.keptByUser).toBe(true);
		expect(ladderText(s.levels)).toBe('L1 · 4 (0/4 done)');
	});

	it('an offline clear and an offline trash read "purges … after this syncs" until the acknowledged rows replace them', async () => {
		const server = new FakeServer();
		server.create({ id: C, source_id: Q, count: 4 });
		const now = Date.now();
		const { db, engine } = await device(server, () => new Date(now));
		const tx = db.transaction(['meta'], 'readwrite');
		await recordOpen(tx as unknown as RwTx, C, new Date(now).toISOString());
		await tx.done;
		await engine.sync();
		// Level 1 descends; Level 2 is cleared offline.
		const q = await grade(db, Q, 'CCCM');
		const l2 = crypto.randomUUID();
		await applyLocally(db, { type: 'finish', quiz_id: Q, attempt: q.attempt, attempt_seed: q.shuffle_seed, shuffle_seed: '3', new_quiz_id: l2 });
		const q2 = await grade(db, l2, 'C');
		await applyLocally(db, { type: 'finish', quiz_id: l2, attempt: q2.attempt, attempt_seed: q2.shuffle_seed, shuffle_seed: '4', new_quiz_id: crypto.randomUUID() });
		let [g] = await trashGroups(db);
		expect(g.entries).toHaveLength(1);
		expect(g.entries[0].provisional).toBe(true);
		expect(g.entries[0].purgesAt).toBeNull();
		// Trashed offline too: in the Trash, gone from the Cascades page.
		await applyLocally(db, { type: 'trash_cascade', cascade_id: C });
		[g] = await trashGroups(db);
		expect(g.trashed).toBe(true);
		expect(g.entries.find((e) => e.kind === 'cascade')!.provisional).toBe(true);
		expect(await cascadeSummaries(db, true)).toEqual([]);
		// Acknowledged: real dates.
		await engine.sync();
		[g] = await trashGroups(db);
		const cascadeEntry = g.entries.find((e) => e.kind === 'cascade')!;
		expect(cascadeEntry.provisional).toBe(false);
		expect(cascadeEntry.purgesAt).not.toBeNull();
	});

	it('refuses on the device a segment size above the cap /api/auth/me reported', async () => {
		const server = new FakeServer();
		server.create({ id: C, source_id: Q, count: 4 });
		const { db, engine } = await device(server, () => new Date());
		await engine.sync();
		await db.put('meta', { trash_retention_days: 30, max_quiz_questions: 100 }, 'server');
		await expect(applyLocally(db, { type: 'set_quiz_options', quiz_id: Q, segment_size: 101 })).rejects.toMatchObject({ reason: 'invalid' });
		await applyLocally(db, { type: 'set_quiz_options', quiz_id: Q, segment_size: 100 });
	});
});
