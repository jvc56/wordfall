// PLAN.md § Unit tests → Frontend: "The rebase tests cover: …" — the sync
// engine, pull and rebase against an in-memory server (testing/fake-server.ts)
// that applies operations through the shared rules.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { describe, expect, it } from 'vitest';
import { resetSeed } from '$lib/cascade/order';
import { applyLocally, APPLY_STORES, type NewOp } from '$lib/local/apply';
import { openUserDb, type UserDb } from '$lib/local/db';
import { getMeta, initMeta, recordOpen } from '$lib/local/meta';
import { preferencesView } from '$lib/local/preferences';
import type { QuizRow } from '$lib/local/rows';
import { storeCreated, type Created } from '$lib/local/created';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { SyncEngine } from './engine';
import type { Notice } from './notices';
import type { Reply, SyncRequest, SyncResponse } from './protocol';
import { FakeServer, type NewCascade } from './testing/fake-server';

const USER = '11111111-1111-4111-8111-111111111111';
const C1 = 'c1000000-0000-4000-8000-000000000000';
const Q1 = 'a1000000-0000-4000-8000-000000000000';

class Dev {
	db!: UserDb;
	engine!: SyncEngine;
	notices: Notice[] = [];
	requests: SyncRequest[] = [];
	/** Stands between the engine and the server: may rewrite the request or the reply. */
	tamper: ((req: SyncRequest, send: (r: SyncRequest) => Reply) => Reply | Promise<Reply>) | null = null;
	online = true;

	static async make(server: FakeServer): Promise<Dev> {
		const d = new Dev();
		globalThis.indexedDB = new IDBFactory();
		d.db = await openUserDb(USER);
		await initMeta(d.db, USER, 'alice');
		d.engine = new SyncEngine(
			d.db,
			{ onNotices: (n) => d.notices.push(...n) },
			{
				transport: async (req) => {
					if (!d.online) throw new TypeError('offline');
					d.requests.push(req);
					const send = (r: SyncRequest) => server.sync(JSON.parse(JSON.stringify(r)));
					return d.tamper ? await d.tamper(req, send) : send(req);
				},
				wait: async () => undefined
			}
		);
		return d;
	}

	async open(cascadeId: string) {
		const tx = this.db.transaction(['meta'], 'readwrite');
		await recordOpen(tx as unknown as RwTx, cascadeId, new Date().toISOString());
		await tx.done;
	}

	private tx() {
		return this.db.transaction(APPLY_STORES) as unknown as RwTx;
	}

	quiz(id: string) {
		return view.quiz(this.tx(), id);
	}

	quizzes(cascadeId: string) {
		return view.quizzesOf(this.tx(), cascadeId);
	}

	rows(quizId: string) {
		return view.questionsOf(this.tx(), quizId);
	}

	apply(op: NewOp) {
		return applyLocally(this.db, op);
	}

	/** Grades the quiz's cards from `from` in position order. */
	async play(quizId: string, marks: string, from = 0) {
		const q = (await this.quiz(quizId))!;
		const byPos = (await this.rows(quizId)).sort((a, b) => a.position! - b.position!);
		for (let i = 0; i < marks.length; i++) {
			await this.apply({
				type: 'grade',
				quiz_id: quizId,
				attempt: q.attempt,
				attempt_seed: q.shuffle_seed,
				question_idx: byPos[from + i].question_idx,
				grade: marks[i] === 'C' ? 'correct' : 'missed'
			});
		}
	}

	async finish(quizId: string, seed = '101', newId = crypto.randomUUID()) {
		const q = (await this.quiz(quizId))!;
		const r = await this.apply({
			type: 'finish',
			quiz_id: quizId,
			attempt: q.attempt,
			attempt_seed: q.shuffle_seed,
			shuffle_seed: seed,
			new_quiz_id: newId
		});
		return { newId, result: r.result };
	}

	async finishSegment(quizId: string, end: number, seed = '202', newId = crypto.randomUUID()) {
		const q = (await this.quiz(quizId))!;
		const r = await this.apply({
			type: 'finish_segment',
			quiz_id: quizId,
			attempt: q.attempt,
			attempt_seed: q.shuffle_seed,
			segment_end: end,
			shuffle_seed: seed,
			new_quiz_id: newId
		});
		return { newId, result: r.result };
	}

	sync() {
		return this.engine.sync();
	}

	async outbox() {
		return this.db.getAll('outbox');
	}
}

/** A server with one cascade, and a device that has opened and pulled it. */
async function setup(c: Partial<NewCascade> = {}) {
	const server = new FakeServer();
	server.create({ id: C1, source_id: Q1, count: 5, ...c });
	const a = await Dev.make(server);
	await a.open(C1);
	await a.sync();
	return { server, a };
}

async function second(server: FakeServer, open = true) {
	const b = await Dev.make(server);
	if (open) await b.open(C1);
	await b.sync();
	return b;
}

const texts = (d: Dev) => d.notices.map((n) => n.text);

let remoteSeq = 1;
/** Another device's operations, sent straight to the server (that device's own rows don't matter here). */
function remote(server: FakeServer, ops: Record<string, unknown>[]) {
	const r = server.sync({
		device_id: 'remote-device',
		app_version: 0,
		cursor: server.seq,
		question_rows_for: [],
		ops: ops.map((o) => ({ id: crypto.randomUUID(), device_seq: remoteSeq++, seen_seq: 0, at: new Date(Date.now() + 1000).toISOString(), ...o }))
	});
	return (r.body as SyncResponse).results;
}

describe('the sync engine', () => {
	it('sends cursor: null on a first sync and builds the Source quiz from a pull that carries no question rows', async () => {
		const { a } = await setup();
		expect(a.requests[0].cursor).toBeNull();
		const rows = await a.rows(Q1);
		expect(rows.map((r) => r.question_idx)).toEqual([0, 1, 2, 3, 4]);
		expect(rows.every((r) => r.position !== null)).toBe(true);
		expect((await getMeta(a.db, 'sync')).cursor).toBeGreaterThan(0);
	});

	it('a pull that only echoes the device’s own operations takes the fast path and empties the overlay', async () => {
		const { a } = await setup();
		await a.play(Q1, 'CCM');
		expect(await a.db.count('overlay_quiz_questions')).toBe(3);
		await a.sync();
		expect(await a.db.count('outbox')).toBe(0);
		expect(await a.db.count('overlay_quiz_questions')).toBe(0);
		expect(await a.db.count('overlay_quizzes')).toBe(0);
		const q = (await a.quiz(Q1))!;
		expect([q.correct_count, q.missed_count]).toEqual([2, 1]);
		expect((await a.rows(Q1)).filter((r) => r.grade !== null)).toHaveLength(3);
	});

	it('the fast path keeps the overlay rows of grades still queued behind the batch', async () => {
		const { a } = await setup({ count: 20 });
		await a.play(Q1, 'CCCCCCCCCC');
		// Send only the first five: the rest stay pending.
		const all = await a.outbox();
		a.tamper = null;
		// The server receives only the first five, as if the batch limit fell there.
		a.tamper = (req, send) => send({ ...req, ops: req.ops!.slice(0, 5) });
		await a.sync();
		a.tamper = null;
		expect(all).toHaveLength(10);
		expect(await a.db.count('outbox')).toBe(5);
		expect(await a.db.count('overlay_quiz_questions')).toBe(5);
	});

	it('an acknowledged offline finish leaves its new level playable with no index-list fetch', async () => {
		const { a } = await setup();
		await a.play(Q1, 'CCCMM');
		const { newId } = await a.finish(Q1);
		await a.sync();
		const rows = await a.rows(newId);
		expect(rows).toHaveLength(2);
		expect(rows.every((r) => r.position !== null)).toBe(true);
		expect(await a.db.get('quiz_questions', [newId, rows[0].question_idx])).toBeDefined();
		expect(await a.db.count('overlay_quiz_questions')).toBe(0);
		expect(a.notices).toEqual([]);
	});

	it('a rejected finish removes the quiz it created from the overlay, with one notice counting what followed', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		// Device B finishes Level 1 first.
		await b.play(Q1, 'CCCMM');
		await b.finish(Q1, '303');
		await b.sync();
		// Device A, offline meanwhile, finished it too and graded its own Level 2.
		await a.play(Q1, 'CCCCM');
		const { newId } = await a.finish(Q1, '404');
		await a.play(newId, 'C');
		await a.sync();
		expect(await a.quiz(newId)).toBeUndefined();
		expect(await a.db.count('outbox')).toBe(0);
		expect(texts(a)).toEqual(['Level 1 was finished on another device. 6 answers from this device weren’t kept.']);
	});

	it('rejected grades on a trashed cascade disappear from the overlay although no server row changed', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await b.apply({ type: 'trash_cascade', cascade_id: C1 });
		await b.sync();
		await a.play(Q1, 'CC');
		// The pull carries the trashed cascade row; the grades' own rows never changed.
		await a.sync();
		expect(await a.db.count('overlay_quiz_questions')).toBe(0);
		expect((await a.rows(Q1)).every((r) => r.grade === null)).toBe(true);
		expect(texts(a)).toEqual(['This cascade was moved to the Trash on another device. 2 answers from this device weren’t kept.']);
	});

	it('a pulled quiz with a new seed is rebuilt, and a reset with no graded rows completes at the last page', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await b.play(Q1, 'MMMMM');
		await b.finish(Q1, '555'); // reshuffled: a reset with no graded rows
		await b.sync();
		const before = (await a.quiz(Q1))!;
		await a.sync();
		const after = (await a.quiz(Q1))!;
		expect(after.attempt).toBe(before.attempt + 1);
		expect(after.shuffle_seed).toBe(resetSeed(555n).toString());
		const rows = await a.rows(Q1);
		expect(rows).toHaveLength(5);
		expect(rows.every((r) => r.grade === null && r.position !== null)).toBe(true);
	});

	it('an applied finish with a different outcome kind shows the notice and no completion', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await a.play(Q1, 'CCCCM');
		const byPos = (await a.rows(Q1)).sort((x, y) => x.position! - y.position!);
		const q = (await b.quiz(Q1))!;
		// B, later and unseen by A, turns A's one miss into correct.
		await b.apply({ type: 'grade', quiz_id: Q1, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: byPos[4].question_idx, grade: 'correct' });
		await b.sync();
		const { newId, result } = await a.finish(Q1);
		expect(result.outcome).toBe('descended');
		await a.sync();
		// The server completed the cascade: the device's Level 2 is gone.
		expect(await a.quiz(newId)).toBeUndefined();
		expect((await a.db.get('cascades', C1))!.completed_at).not.toBeNull();
		expect(texts(a)).toContain('Level 1 came out differently on the server because of changes made on another device.');
	});

	it('an applied finish differing only in the new quiz’s question count says nothing about it', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await a.play(Q1, 'CCCCM');
		const byPos = (await a.rows(Q1)).sort((x, y) => x.position! - y.position!);
		const q = (await b.quiz(Q1))!;
		await b.apply({ type: 'grade', quiz_id: Q1, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: byPos[0].question_idx, grade: 'missed' });
		await b.sync();
		const { newId } = await a.finish(Q1);
		await a.sync();
		// Descended on both sides; the server's Level 2 has two questions, the device's one.
		expect((await a.quiz(newId))!.question_count).toBe(2);
		expect(texts(a).some((t) => t.includes('came out differently'))).toBe(false);
	});

	it('a finish_segment the device computed as drilled but the server applied as continued drops the drill quiz with no request', async () => {
		const { server, a } = await setup({ count: 10, segment_size: 5 });
		const b = await second(server);
		// B regrades A's miss as correct before A's run finish arrives.
		await a.play(Q1, 'CCCCM');
		const byPos = (await a.rows(Q1)).sort((x, y) => x.position! - y.position!);
		const missed = byPos[4].question_idx;
		const { newId } = await a.finishSegment(Q1, 5);
		const q = (await b.quiz(Q1))!;
		await b.apply({ type: 'grade', quiz_id: Q1, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: missed, grade: 'correct' });
		// B's grade is later and unseen by A: A's own grade on it is stale; the
		// run then missed nothing on the server.
		await b.sync();
		await a.sync();
		expect(await a.quiz(newId)).toBeUndefined();
		expect(a.requests.every((r) => r.page_token === undefined)).toBe(true);
		expect(texts(a)[0]).toMatch(/^Level 1 came out differently on the server because of changes made on another device\./);
	});

	it('a stale-rejected preferences change reverts the view; a pull while one is pending leaves it alone', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await a.apply({ type: 'set_preferences', default_clear_threshold: 60 });
		await b.apply({ type: 'set_preferences', leave_value_decimals: 3 });
		await b.sync();
		// A pull arrives while A's change is still pending: the view keeps it.
		const pulled = { ...(await preferencesView(a.db)), default_clear_threshold: 90 };
		await a.db.put('preferences', pulled, 'row');
		expect((await preferencesView(a.db)).default_clear_threshold).toBe(60);
		// A's change is older and unseen: stale; the view reverts to the server's row.
		await a.sync();
		expect((await preferencesView(a.db)).default_clear_threshold).toBe(80);
		expect((await preferencesView(a.db)).leave_value_decimals).toBe(3);
	});

	it('a cascade outside the window holds no question rows, and its played quizzes are pending', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 5 });
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		await a.play(Q1, 'CC');
		await a.sync();
		const b = await second(server, false);
		expect(await b.db.countFromIndex('quiz_questions', 'cascade_id', C1)).toBe(0);
		expect((await b.db.get('quizzes', Q1))!.pending).toBe(true);
		expect(b.requests[0].question_rows_for).toEqual([]);
	});

	it('an outbox of more than one batch with its finish dropped in replay shows one stale_attempt notice', async () => {
		// The shape of the 1,200-operation vector: the finish lies past the first
		// batch, so the server rejects the grades before it and the replay drops
		// the finish and everything built on it.
		const { server, a } = await setup({ count: 600 });
		const b = await second(server);
		await b.play(Q1, 'C'.repeat(599) + 'M');
		await b.finish(Q1, '9');
		await b.sync();
		await a.play(Q1, 'C'.repeat(200) + 'M'.repeat(400));
		const { newId } = await a.finish(Q1, '10');
		await a.play(newId, 'C'.repeat(400));
		await a.finish(newId, '11');
		expect(await a.db.count('outbox')).toBe(1002);
		await a.sync();
		expect(await a.db.count('outbox')).toBe(0);
		expect(texts(a)).toEqual(['Level 1 was finished on another device. 1000 answers from this device weren’t kept.']);
	}, 300_000);

	it('two offline finishes in one batch make one not_deepest notice counting both levels', async () => {
		const { server, a } = await setup({ count: 10, segment_size: 5 });
		const b = await second(server);
		// B finishes a run of Level 1 with a miss: Level 2 appears below it.
		await b.play(Q1, 'CCCCM');
		await b.finishSegment(Q1, 5, '1');
		await b.sync();
		// A, offline, grades Level 1 through and finishes it, then plays and
		// finishes the Level 2 that made.
		await a.play(Q1, 'CCCCCCCMMM');
		const l2 = await a.finish(Q1, '2');
		await a.play(l2.newId, 'CCM');
		await a.finish(l2.newId, '3');
		await a.sync();
		expect(texts(a)).toEqual([
			'Another level was added below Level 1 on another device. Finish it first. 3 answers from this device weren’t kept.'
		]);
	});

	it('503 resends the same batch after Retry-After; 429 backs off; the outbox stays intact', async () => {
		const { server, a } = await setup();
		await a.play(Q1, 'C');
		server.forced.push({ status: 503, body: {}, retryAfter: 1 }, { status: 429, body: {}, retryAfter: 2 });
		await a.sync();
		const sent = a.requests.slice(-3).map((r) => r.ops!.length);
		expect(sent).toEqual([1, 1, 1]);
		expect(await a.db.count('outbox')).toBe(0);
	});

	it('a 426 acknowledges its operations and says reload to keep syncing until a sync succeeds', async () => {
		const { server, a } = await setup();
		await a.play(Q1, 'CC');
		a.tamper = (req, send) => {
			const r = send(req);
			return req.ops?.length ? { ...r, status: 426 } : r;
		};
		await a.sync();
		expect(a.engine.status).toBe('reload');
		expect(await a.db.count('outbox')).toBe(0);
		expect((await getMeta(a.db, 'sync')).upgrade_required).toBe(true);
		a.tamper = null;
		void server;
		await a.play(Q1, 'C', 2);
		await a.sync();
		expect((await getMeta(a.db, 'sync')).upgrade_required).toBe(false);
	});

	it('resync_required drops the acknowledged operations, replaces the base from a full pull and keeps the rest', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await b.play(Q1, 'CCCCC');
		await b.finish(Q1, '7');
		await b.sync();
		// Tombstones are pruned past the device's cursor.
		server.create({ id: 'c2000000-0000-4000-8000-000000000000', source_id: 'a2000000-0000-4000-8000-000000000000', count: 3 });
		server.floor = server.seq + 1;
		const before = (await a.rows(Q1)).map((r) => r.position);
		await a.play(Q1, 'C');
		await a.sync();
		const nulls = a.requests.filter((r) => r.cursor === null);
		expect(nulls.length).toBe(2); // the first sync and the resync
		expect(await a.db.count('outbox')).toBe(0);
		expect((await a.rows(Q1)).map((r) => r.position)).not.toEqual(before); // B's finish reset it
		expect(await a.db.get('cascades', 'c2000000-0000-4000-8000-000000000000')).toBeDefined();
	});

	it('a full pull deletes a quiz it did not carry and spares a cascade created while it was in flight', async () => {
		const { server, a } = await setup();
		// A stale base row the server no longer has.
		await a.db.put('quizzes', { ...((await a.db.get('quizzes', Q1)) as QuizRow), id: 'dead0000-0000-4000-8000-000000000000' });
		await a.db.put('meta', { cursor: null, last_sync_at: null }, 'sync');
		// A cascade created through POST /api/cascades while the full pull is in
		// flight: its rows carry the creation's sequence, above the pull's.
		server.pageRows = 1;
		let created = false;
		a.tamper = async (req, send) => {
			if (req.page_token && !created) {
				created = true;
				const c = server.create({ id: 'c3000000-0000-4000-8000-000000000000', source_id: 'a3000000-0000-4000-8000-000000000000', count: 2 });
				await storeCreated(a.db, c as unknown as Created);
			}
			return send(req);
		};
		await a.sync();
		expect(created).toBe(true);
		expect(await a.db.get('quizzes', 'dead0000-0000-4000-8000-000000000000')).toBeUndefined();
		expect(await a.db.get('cascades', 'c3000000-0000-4000-8000-000000000000')).toBeDefined();
	});

	it('an interrupted pull leaves the cursor unadvanced and the next sync converges', async () => {
		const { server, a } = await setup({ count: 30 });
		const b = await second(server);
		await b.play(Q1, 'C'.repeat(12));
		await b.sync();
		server.pageRows = 4;
		const cursor = (await getMeta(a.db, 'sync')).cursor;
		let n = 0;
		a.tamper = (req, send) => (req.page_token && ++n === 2 ? { status: 500, body: {}, retryAfter: null } : send(req));
		await a.sync();
		expect((await getMeta(a.db, 'sync')).cursor).toBe(cursor);
		a.tamper = null;
		await a.sync();
		expect((await a.rows(Q1)).filter((r) => r.grade === 'correct')).toHaveLength(12);
	});

	it('an applied purge_cascade takes the fast path with its tombstone applied', async () => {
		const { a } = await setup();
		await a.apply({ type: 'trash_cascade', cascade_id: C1 });
		await a.apply({ type: 'purge_cascade', cascade_id: C1 });
		await a.sync();
		expect(await a.db.get('cascades', C1)).toBeUndefined();
		expect(await a.db.countFromIndex('quiz_questions', 'cascade_id', C1)).toBe(0);
		expect(await getMeta(a.db, 'opens')).toEqual({});
	});

	it('a stale grade beside an applied finish with a matching outcome shows the grade’s own sentence', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		const byPos = (await b.rows(Q1)).sort((x, y) => x.position! - y.position!);
		const q = (await b.quiz(Q1))!;
		await a.play(Q1, 'CCCCM');
		await b.apply({ type: 'grade', quiz_id: Q1, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: byPos[0].question_idx, grade: 'missed' });
		await b.sync();
		await a.finish(Q1);
		await a.sync();
		expect(texts(a)).toContain('Level 1 was also answered on another device. 1 answer from this device wasn’t kept.');
	});

	it('a restore_quiz rejected with reason error drops its overlay quiz row and shows the could-not-be-restored notice', async () => {
		const { a } = await setup();
		await a.play(Q1, 'CCCCM');
		const { newId } = await a.finish(Q1);
		await a.play(newId, 'C');
		await a.finish(newId);
		await a.sync();
		await a.apply({ type: 'restore_quiz', quiz_id: newId, shuffle_seed: '77' });
		// The server rolls the restore's savepoint back and records it as `error`.
		a.tamper = (req, send) => {
			if (!req.ops?.length) return send(req);
			const r = send({ ...req, ops: [] });
			const body = r.body as SyncResponse;
			const results = req.ops.map((o) => ({ op_id: (o as { id: string }).id, status: 'rejected' as const, reason: 'error' }));
			return { ...r, body: { ...body, results } };
		};
		await a.sync();
		expect((await a.quiz(newId))!.status).toBe('cleared');
		expect(texts(a)).toEqual(['This quiz could not be restored on the server.']);
	});
});


describe('the rebase, continued', () => {
	it('a finish acknowledged on its second send, after a lost response, promotes its rows with no notice', async () => {
		const { a } = await setup();
		await a.play(Q1, 'CCCMM');
		const { newId } = await a.finish(Q1);
		// The server applies the batch but the response is lost.
		a.tamper = (req, send) => {
			send(req);
			throw new TypeError('connection reset');
		};
		await a.sync();
		expect(a.engine.status).toBe('offline');
		a.tamper = null;
		await a.sync();
		expect((await a.rows(newId)).every((r) => r.position !== null)).toBe(true);
		expect(await a.db.count('overlay_quiz_questions')).toBe(0);
		expect(a.notices).toEqual([]);
	});

	it('a finish the device computed as cleared but the server descended leaves the quiz rebuilt from the server’s seed', async () => {
		const { server, a } = await setup({ count: 5 });
		// Level 2 of three questions on both devices.
		await a.play(Q1, 'CCMMM');
		const { newId: l2 } = await a.finish(Q1);
		await a.sync();
		const byPos = (await a.rows(l2)).sort((x, y) => x.position! - y.position!);
		await a.play(l2, 'CCC');
		const q = (await a.quiz(l2))!;
		// Another device, later, marks two of them missed: the server's score is 1/3 and it descends.
		remote(
			server,
			byPos.slice(0, 2).map((r) => ({ type: 'grade', quiz_id: l2, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: r.question_idx, grade: 'missed' }))
		);
		const { result } = await a.finish(l2, '808');
		expect(result.outcome).toBe('cleared');
		await a.sync();
		const after = (await a.quiz(l2))!;
		expect(after.status).toBe('active');
		expect(after.shuffle_seed).toBe(resetSeed(808n).toString());
		expect((await a.rows(l2)).every((r) => r.grade === null && r.position !== null)).toBe(true);
		expect(texts(a)).toContain('Level 2 came out differently on the server because of changes made on another device.');
	});

	it('the reverse leaves the base’s graded rows of the cleared quiz untouched, with no fetch', async () => {
		const { server, a } = await setup({ count: 5 });
		await a.play(Q1, 'CCMMM');
		const { newId: l2 } = await a.finish(Q1);
		await a.sync();
		const byPos = (await a.rows(l2)).sort((x, y) => x.position! - y.position!);
		await a.play(l2, 'CMM');
		await a.sync();
		const q = (await a.quiz(l2))!;
		remote(
			server,
			byPos.slice(1).map((r) => ({ type: 'grade', quiz_id: l2, attempt: 1, attempt_seed: q.shuffle_seed, question_idx: r.question_idx, grade: 'correct' }))
		);
		const { result } = await a.finish(l2, '909');
		expect(result.outcome).toBe('descended');
		const pages = a.requests.length;
		await a.sync();
		expect((await a.quiz(l2))!.status).toBe('cleared');
		// Its rows and positions stay in the base, for a Trash export offline.
		const rows = await a.rows(l2);
		expect(rows).toHaveLength(3);
		expect(rows.every((r) => r.position !== null && r.grade !== null)).toBe(true);
		expect(a.requests.length - pages).toBe(1);
	});

	it('grade rows pulled for a pending quiz are written nowhere, and a pull that reports it cleared clears the flag', async () => {
		const { server, a } = await setup({ count: 5 });
		const b = await second(server, false);
		await a.play(Q1, 'CCMMM');
		const { newId: l2 } = await a.finish(Q1);
		await a.play(l2, 'C');
		await a.sync();
		await b.sync();
		expect((await b.db.get('quizzes', l2))!.pending).toBe(true);
		// B opens the cascade: it is now in question_rows_for, but Level 2 stays pending.
		await b.open(C1);
		await a.play(l2, 'C', 1);
		await a.sync();
		await b.sync();
		expect(b.requests.at(-1)!.question_rows_for).toEqual([C1]);
		expect(await b.db.countFromIndex('quiz_questions', 'quiz_id', l2)).toBe(0);
		expect((await b.db.get('quizzes', l2))!.pending).toBe(true);
		await a.play(l2, 'C', 2);
		await a.finish(l2);
		await a.sync();
		await b.sync();
		const row = (await b.db.get('quizzes', l2))!;
		expect(row.status).toBe('cleared');
		expect(row.pending).toBeUndefined();
	});

	it('a full pull deletes no base row of a cascade with pending operations', async () => {
		const { server, a } = await setup();
		await a.play(Q1, 'C');
		// The server loses the cascade (another device purged it); A still has an operation queued.
		server.cascades.delete(C1);
		a.tamper = (req, send) => send({ ...req, ops: [] });
		await a.db.put('meta', { cursor: null, last_sync_at: null }, 'sync');
		await a.sync();
		expect(await a.db.get('cascades', C1)).toBeDefined();
		expect(await a.db.count('outbox')).toBe(1);
	});

	it('a finish rejected not_active, and grades rejected stale_attempt with no finish behind them, say it was finished elsewhere', async () => {
		const { server, a } = await setup();
		await a.play(Q1, 'CCMMM');
		const { newId: l2 } = await a.finish(Q1);
		await a.sync();
		const b = await second(server);
		const q2 = (await a.quiz(l2))!;
		const idx = (await a.rows(l2)).map((r) => r.question_idx);
		// Another device grades Level 2 through and clears it.
		remote(server, [
			...idx.map((i) => ({ type: 'grade', quiz_id: l2, attempt: 1, attempt_seed: q2.shuffle_seed, question_idx: i, grade: 'correct' })),
			{ type: 'finish', quiz_id: l2, attempt: 1, attempt_seed: q2.shuffle_seed, shuffle_seed: '31', new_quiz_id: crypto.randomUUID() }
		]);
		await a.play(l2, 'CCC');
		await a.finish(l2);
		await a.sync();
		expect(texts(a)).toEqual(['Level 2 was finished on another device. 3 answers from this device weren’t kept.']);
		// Grades alone, on a quiz another device has since reset.
		await b.sync();
		await b.play(Q1, 'MMMMM');
		await b.finish(Q1, '5');
		await b.sync();
		a.notices = [];
		await a.play(Q1, 'CC');
		await a.sync();
		expect(texts(a)).toEqual(['Level 1 was finished on another device. 2 answers from this device weren’t kept.']);
	});

	it('not_found on a purged cascade says it was deleted; not_found for a question the server’s level lacks says it came out differently', async () => {
		const { server, a } = await setup();
		const b = await second(server);
		await b.apply({ type: 'trash_cascade', cascade_id: C1 });
		await b.apply({ type: 'purge_cascade', cascade_id: C1 });
		await b.sync();
		await a.play(Q1, 'CC');
		await a.sync();
		expect(texts(a)).toEqual([
			'This cascade was deleted, on another device or by the Trash’s retention period. 2 answers from this device weren’t kept.'
		]);
		expect(await a.db.get('cascades', C1)).toBeUndefined();
	});

	it('a full segmented attempt pushed as the device emits it has no bad_cursor or bad_segment and its finish applies', async () => {
		const { server, a } = await setup({ count: 12, segment_size: 5 });
		/** Plays a quiz as the player emits it: Show / Next saves the grade and
		 * moves the cursor, except at the last card of a run (finish_segment) or of
		 * the attempt (finish); a drilled run's level is played before its parent goes on. */
		const playOut = async (quizId: string, miss: (p: number) => boolean): Promise<void> => {
			for (;;) {
				const q = (await a.quiz(quizId))!;
				if (q.status !== 'active') return;
				const byPos = (await a.rows(quizId)).sort((x, y) => x.position! - y.position!);
				const p = q.cursor;
				await a.apply({
					type: 'grade',
					quiz_id: quizId,
					attempt: q.attempt,
					attempt_seed: q.shuffle_seed,
					question_idx: byPos[p].question_idx,
					grade: miss(p) && q.level === 1 ? 'missed' : 'correct'
				});
				const size = q.segment_chain ? 0 : q.segment_size;
				if (p + 1 === q.question_count) {
					await a.finish(quizId, String(p) + q.level);
					return;
				}
				if (size && (p + 1) % size === 0) {
					const r = await a.finishSegment(quizId, p + 1, String(p));
					if (r.result.outcome === 'drilled') await playOut(r.newId, () => false);
					continue;
				}
				await a.apply({ type: 'move_cursor', quiz_id: quizId, attempt: q.attempt, attempt_seed: q.shuffle_seed, position: p + 1 });
			}
		};
		await playOut(Q1, (p) => p % 3 === 0);
		await a.sync();
		const reasons = [...server.records.values()].filter((r) => r.status === 'rejected').map((r) => r.reason);
		expect(reasons).toEqual([]);
		expect(a.notices).toEqual([]);
	});

	it('every base row carries the sequence of the response that wrote it', async () => {
		const { a } = await setup();
		await a.play(Q1, 'C');
		await a.sync();
		const cursor = (await getMeta(a.db, 'sync')).cursor!;
		const graded = (await a.db.getAll('quiz_questions')).filter((r) => r.grade !== null);
		expect(graded.map((r) => r.seq)).toEqual([cursor]);
		expect((await a.db.get('quizzes', Q1))!.seq).toBe(cursor);
	});

	it('the engine drains a 600-batch outbox back to back', async () => {
		const { server, a } = await setup();
		const entries = [];
		for (let i = 1; i <= 600 * 500; i++) {
			entries.push({ device_seq: i, op: { id: `op-${i}`, device_seq: i, seen_seq: 0, at: '2026-01-01T00:00:00.000Z', type: 'set_preferences', default_clear_threshold: 50 } });
		}
		const tx = a.db.transaction(['outbox', 'meta'], 'readwrite');
		for (const e of entries) void tx.objectStore('outbox').put(e);
		await tx.done;
		const before = a.requests.length;
		await a.sync();
		expect(a.requests.length - before).toBe(600);
		expect(await a.db.count('outbox')).toBe(0);
		void server;
	}, 600_000);
});

describe('authentication while offline', () => {
	it('a 401 keeps the data and the outbox and says log in to sync; studying continues', async () => {
		const { a } = await setup();
		await a.play(Q1, 'CC');
		a.tamper = () => ({ status: 401, body: { error: 'unauthorized' }, retryAfter: null });
		await a.sync();
		expect(a.engine.status).toBe('needs_login');
		expect(await a.db.count('outbox')).toBe(2);
		await a.play(Q1, 'C', 2);
		expect(await a.db.count('outbox')).toBe(3);
		a.tamper = null;
		await a.sync();
		expect(await a.db.count('outbox')).toBe(0);
	});
});
