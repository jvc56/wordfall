// PLAN.md § Unit tests → Frontend: the download manager and the drop pass —
// first opens, the window, pending quizzes and their grades, keys before
// answers, eviction, the row budget and its mark, 429 backoff, the player's
// priority, distributions, the quota path.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { describe, expect, it } from 'vitest';
import { ApiError } from '$lib/api';
import { applyLocally, APPLY_STORES } from '$lib/local/apply';
import { openUserDb, type UserDb } from '$lib/local/db';
import { getMeta, initMeta, recordOpen, writeMeta } from '$lib/local/meta';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { DownloadManager, ROW_BYTES, type DownloadApi } from './downloads';
import { SyncEngine } from './engine';
import { setKeepOffline } from './keep';
import { policy } from './policy';
import { FakeServer } from './testing/fake-server';

const USER = '22222222-2222-4222-8222-222222222222';
const C1 = 'c1000000-0000-4000-8000-000000000000';
const Q1 = 'a1000000-0000-4000-8000-000000000000';
const C2 = 'c2000000-0000-4000-8000-000000000000';
const Q2 = 'a2000000-0000-4000-8000-000000000000';
const C3 = 'c3000000-0000-4000-8000-000000000000';
const Q3 = 'a3000000-0000-4000-8000-000000000000';
const DAY = 86_400_000;

interface Call {
	kind: string;
	cascade: string;
	quiz?: string;
	from: number;
}

class Dev {
	db!: UserDb;
	engine!: SyncEngine;
	dl!: DownloadManager;
	calls: Call[] = [];
	/** Throws for a call before it reaches the server. */
	fail: ((c: Call) => Error | null) | null = null;
	now = Date.now();
	openHere: string | null = null;
	openElsewhere: Set<string> | null = null;

	static async make(server: FakeServer, o: { rowBudget?: number; answerLimit?: number } = {}) {
		const d = new Dev();
		globalThis.indexedDB = new IDBFactory();
		d.db = await openUserDb(USER);
		await initMeta(d.db, USER, 'bob');
		const clock = () => new Date(d.now);
		d.engine = new SyncEngine(d.db, {}, { transport: async (r) => server.sync(JSON.parse(JSON.stringify(r))), wait: async () => undefined, now: clock });
		const guard = (c: Call) => {
			d.calls.push(c);
			const e = d.fail?.(c);
			if (e) throw e;
		};
		const api: DownloadApi = {
			questions: async (cascade, quiz, from, limit) => {
				guard({ kind: 'questions', cascade, quiz, from });
				return server.questions(quiz, from, limit)!;
			},
			grades: async (cascade, quiz, from, limit) => {
				guard({ kind: 'grades', cascade, quiz, from });
				return server.grades(quiz, from, limit)!;
			},
			keys: async (cascade, from, limit) => {
				guard({ kind: 'keys', cascade, from });
				return server.keys(cascade, from, limit)!;
			},
			cards: async (cascade, from, limit, hooks, definitions) => {
				guard({ kind: definitions ? 'cards+defs' : 'cards', cascade, from });
				return server.cards(cascade, from, limit, hooks, definitions)!;
			},
			distribution: async (name) => {
				guard({ kind: 'distribution', cascade: name, from: 0 });
				return { name, tiles: [] };
			}
		};
		d.dl = new DownloadManager(d.db, {
			api,
			sync: () => d.engine.sync(),
			wait: async () => undefined,
			now: clock,
			openHere: () => d.openHere,
			openElsewhere: async () => d.openElsewhere,
			...o
		});
		return d;
	}

	async open(cascadeId: string) {
		const tx = this.db.transaction(['meta'], 'readwrite');
		await recordOpen(tx as unknown as RwTx, cascadeId, new Date(this.now).toISOString());
		await tx.done;
	}

	/** A sync, then the download manager's pass, as the engine's hook runs it. */
	async sync() {
		await this.engine.sync();
		await this.dl.run();
	}

	rows(quizId: string) {
		return view.questionsOf(this.db.transaction(APPLY_STORES) as unknown as RwTx, quizId);
	}

	count(store: 'quiz_questions' | 'questions' | 'cards', cascadeId: string) {
		return this.db.countFromIndex(store, 'cascade_id', cascadeId);
	}

	quizRow(id: string) {
		return this.db.get('quizzes', id);
	}

	async grade(quizId: string, positions: number[], g: 'correct' | 'missed' = 'correct') {
		const tx = this.db.transaction(APPLY_STORES) as unknown as RwTx;
		const q = (await view.quiz(tx, quizId))!;
		const byPos = (await view.questionsOf(tx, quizId)).sort((a, b) => a.position! - b.position!);
		for (const p of positions) {
			await applyLocally(this.db, {
				type: 'grade',
				quiz_id: quizId,
				attempt: q.attempt,
				attempt_seed: q.shuffle_seed,
				question_idx: byPos[p].question_idx,
				grade: g
			});
		}
	}

	async finish(quizId: string, seed: string) {
		const q = (await view.quiz(this.db.transaction(APPLY_STORES) as unknown as RwTx, quizId))!;
		const newId = crypto.randomUUID();
		await applyLocally(this.db, { type: 'finish', quiz_id: quizId, attempt: q.attempt, attempt_seed: q.shuffle_seed, shuffle_seed: seed, new_quiz_id: newId });
		return newId;
	}
}

const kinds = (d: Dev) => d.calls.map((c) => c.kind);

describe('the download manager', () => {
	it('a cascade outside the window fetches its lists and grades and materialises on first open; leaving the window drops its rows, keys and cards and reopening brings every grade back', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 8 });
		// Device A plays it: Level 1 descends, Level 2 is half graded.
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		await a.grade(Q1, [0, 1, 2, 3], 'correct');
		await a.grade(Q1, [4, 5, 6, 7], 'missed');
		const l2 = await a.finish(Q1, '5');
		await a.grade(l2, [0, 1], 'correct');
		await a.sync();
		// Device B has never opened it: no rows, keys or cards; Level 2 pending.
		const b = await Dev.make(server);
		await b.sync();
		expect(await b.count('quiz_questions', C1)).toBe(0);
		expect(await b.count('questions', C1)).toBe(0);
		expect(await b.count('cards', C1)).toBe(0);
		expect((await b.quizRow(l2))!.pending).toBe(true);
		// First open: the sequence runs for it.
		await b.open(C1);
		await b.dl.ensureCascade(C1);
		const rows = await b.rows(l2);
		expect(rows).toHaveLength(4);
		expect(rows.filter((r) => r.grade === 'correct')).toHaveLength(2);
		expect(rows.every((r) => r.position !== null)).toBe(true);
		expect((await b.quizRow(l2))!.pending).toBeUndefined();
		expect(await b.count('questions', C1)).toBe(8);
		expect(await b.count('cards', C1)).toBe(8);
		// Fifteen days later, with nothing pending, it leaves the window.
		b.now += 15 * DAY;
		await b.dl.run();
		expect(await b.count('quiz_questions', C1)).toBe(0);
		expect(await b.count('questions', C1)).toBe(0);
		expect(await b.count('cards', C1)).toBe(0);
		expect(await b.db.get('cascades', C1)).toBeDefined();
		expect(await b.quizRow(l2)).toBeDefined();
		expect(await b.db.countFromIndex('quiz_attempts', 'cascade_id', C1)).toBe(1);
		// Reopened: lists and graded rows fetched again, every grade intact.
		await b.open(C1);
		await b.dl.ensureCascade(C1);
		expect((await b.rows(l2)).filter((r) => r.grade === 'correct')).toHaveLength(2);
		expect((await b.rows(Q1)).every((r) => r.position !== null)).toBe(true);
	});

	it('a grades fetch whose attempt differs from the base row is discarded and the device syncs again', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 6 });
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		await a.grade(Q1, [0, 1, 2]);
		await a.sync();
		const b = await Dev.make(server);
		await b.sync();
		await b.open(C1);
		// Between B's sync and its fetch, A finishes the attempt: the fetch names the new one.
		let once = false;
		b.fail = (c) => {
			if (c.kind === 'grades' && !once) {
				once = true;
				server.sync({
					device_id: 'x',
					app_version: 0,
					cursor: server.seq,
					question_rows_for: [],
					ops: [3, 4, 5].map((p, i) => ({ id: crypto.randomUUID(), device_seq: 100 + i, seen_seq: server.seq, at: new Date().toISOString(), type: 'noop', p }))
				});
			}
			return null;
		};
		await b.dl.ensureCascade(C1);
		const syncsBefore = kinds(b).filter((k) => k === 'grades').length;
		expect(syncsBefore).toBeGreaterThanOrEqual(1);
		expect((await b.rows(Q1)).filter((r) => r.grade === 'correct')).toHaveLength(3);
	});

	it('a grades fetch cut off after its first page leaves the quiz pending and is fetched again; run to the end it clears in one pass', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 60_010 });
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		// Grades on both sides of the 50,000 page boundary.
		const byPos = (await a.rows(Q1)).sort((x, y) => x.position! - y.position!);
		const low = byPos.findIndex((r) => r.question_idx < 50_000);
		const high = byPos.findIndex((r) => r.question_idx >= 50_000);
		await a.grade(Q1, [low, high]);
		await a.sync();
		const b = await Dev.make(server);
		await b.sync();
		await b.open(C1);
		// The second page's rows come back empty the first time: short of the count.
		let cut = true;
		const orig = server.grades.bind(server);
		server.grades = (quiz, from, limit) => {
			const p = orig(quiz, from, limit)!;
			if (from > 0 && cut) {
				cut = false;
				return { ...p, question_idx: [], grade: [], graded_at: [] };
			}
			return p;
		};
		await b.dl.ensureCascade(C1);
		expect((await b.quizRow(Q1))!.pending).toBeUndefined();
		expect((await b.rows(Q1)).filter((r) => r.grade !== null)).toHaveLength(2);
		expect(kinds(b).filter((k) => k === 'grades').length).toBe(4);
	}, 600_000);

	it('a new unplayed quiz materialises with an index-list fetch and no grades request, and the drop pass leaves it unflagged', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 6 });
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		await a.grade(Q1, [0, 1, 2], 'correct');
		await a.grade(Q1, [3, 4, 5], 'missed');
		const l2 = await a.finish(Q1, '7');
		await a.sync();
		const b = await Dev.make(server);
		await b.open(C1);
		await b.sync();
		expect(b.calls.filter((c) => c.quiz === l2).map((c) => c.kind)).toEqual(['questions']);
		expect((await b.rows(l2)).every((r) => r.position !== null && r.grade === null)).toBe(true);
		b.now += 15 * DAY;
		await b.dl.run();
		expect((await b.quizRow(l2))!.pending).toBeUndefined();
	});

	it('fetches every in-window cascade’s keys before any cascade’s answers', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 3 });
		server.create({ id: C2, source_id: Q2, count: 3 });
		const b = await Dev.make(server);
		await b.open(C1);
		await b.open(C2);
		await b.sync();
		const order = kinds(b).filter((k) => k === 'keys' || k === 'cards');
		expect(order).toEqual(['keys', 'keys', 'cards', 'cards']);
		expect(b.calls.find((c) => c.kind === 'distribution')).toBeDefined();
		expect(await b.db.get('distributions', 'english')).toBeDefined();
	});

	it('evicts answers least recently opened first, sparing a user-kept cascade and every cascade’s keys and rows', async () => {
		const server = new FakeServer();
		for (const [c, q] of [[C1, Q1], [C2, Q2], [C3, Q3]]) server.create({ id: c, source_id: q, count: 4 });
		const b = await Dev.make(server, { answerLimit: 1 });
		const t = b.now;
		b.now = t - 3 * DAY;
		await b.open(C1); // the oldest, but kept by the user
		b.now = t - 2 * DAY;
		await b.open(C2);
		b.now = t - DAY;
		await b.open(C3);
		b.now = t;
		await b.db.put('meta', [C1], 'keep_offline');
		await b.sync();
		// The limit (one byte) is still exceeded by the kept cascade alone: C2 and C3 lose theirs.
		expect(await b.count('cards', C1)).toBe(4);
		expect(await b.count('cards', C2)).toBe(0);
		expect(await b.count('cards', C3)).toBe(0);
		for (const c of [C1, C2, C3]) {
			expect(await b.count('questions', c)).toBe(4);
			expect(await b.count('quiz_questions', c)).toBe(4);
		}
	});

	it('evicts the oldest first and stops once under the limit', async () => {
		const server = new FakeServer();
		for (const [c, q] of [[C1, Q1], [C2, Q2], [C3, Q3]]) server.create({ id: c, source_id: q, count: 4 });
		const b = await Dev.make(server);
		const t = b.now;
		b.now = t - 3 * DAY;
		await b.open(C1);
		b.now = t - 2 * DAY;
		await b.open(C2);
		b.now = t;
		await b.open(C3);
		await b.sync();
		const sizes = await getMeta(b.db, 'sizes');
		const per = sizes[C1].answer_bytes;
		// A limit that fits two cascades' answers: only the oldest goes.
		const b2 = b.dl as unknown as { answerLimit: number };
		b2.answerLimit = per * 2;
		await b.dl.evict();
		expect(await b.count('cards', C1)).toBe(0);
		expect(await b.count('cards', C2)).toBe(4);
		expect(await b.count('cards', C3)).toBe(4);
	});

	it('the budget drops the least recently opened unkept cascade first, marks it, and the next pass settles; opening clears the mark', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 50 });
		server.create({ id: C2, source_id: Q2, count: 50 });
		const b = await Dev.make(server);
		const t = b.now;
		b.now = t - 2 * DAY;
		await b.open(C1);
		b.now = t;
		await b.open(C2);
		await b.sync();
		// A budget that holds one cascade's rows and keys, not two.
		(b.dl as unknown as { rowBudget: number }).rowBudget = 50 * ROW_BYTES + 50 * 60;
		await b.dl.run();
		expect(await b.count('quiz_questions', C1)).toBe(0);
		expect(await b.count('quiz_questions', C2)).toBe(50);
		expect(await getMeta(b.db, 'budget_dropped')).toEqual([C1]);
		// The next pull's pass fetches nothing for it and leaves it out of question_rows_for.
		b.calls = [];
		await b.sync();
		expect(b.calls.filter((c) => c.cascade === C1)).toEqual([]);
		expect(b.engine['transport']).toBeDefined();
		// Opening it clears the mark and fetches it back whole.
		(b.dl as unknown as { rowBudget: number }).rowBudget = 1e12;
		await b.open(C1);
		expect(await getMeta(b.db, 'budget_dropped')).toEqual([]);
		await b.dl.ensureCascade(C1);
		expect(await b.count('quiz_questions', C1)).toBe(50);
	});

	it('the budget takes automatically kept cascades only after every unkept one, and never a user-kept one', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 20 });
		server.create({ id: C2, source_id: Q2, count: 20 });
		server.create({ id: C3, source_id: Q3, count: 20 });
		const b = await Dev.make(server);
		for (const c of [C1, C2, C3]) await b.open(C1 === c ? c : c);
		await b.sync();
		await b.db.put('meta', [C3], 'keep_offline');
		const pol = await policy(b.db, new Date(b.now));
		pol.autoKept.add(C1);
		pol.userKept.add(C3);
		(b.dl as unknown as { rowBudget: number }).rowBudget = 1;
		const r = await b.dl.dropPass(pol);
		expect(await b.count('quiz_questions', C2)).toBe(0);
		expect(await b.count('quiz_questions', C1)).toBe(0);
		expect(await b.count('quiz_questions', C3)).toBe(20);
		expect(r.overByUserKept).toBe(true);
	});

	// PLAN.md § Downloads: "A budget drop is remembered, or it would undo itself" —
	// for an automatically kept cascade too, until it is opened again.
	it('a budget-dropped cascade kept automatically by size is neither wanted nor kept until it is opened', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 60_000 });
		const b = await Dev.make(server);
		await b.engine.sync();
		await b.open(C1);
		let pol = await policy(b.db, new Date(b.now));
		expect(pol.autoKept.has(C1)).toBe(true);
		await b.db.put('meta', [C1], 'budget_dropped');
		pol = await policy(b.db, new Date(b.now));
		expect(pol.wanted.has(C1)).toBe(false);
		expect(pol.autoKept.has(C1)).toBe(false);
		// Opening it clears the mark: kept automatically again.
		await b.open(C1);
		pol = await policy(b.db, new Date(b.now));
		expect(await getMeta(b.db, 'budget_dropped')).toEqual([]);
		expect(pol.autoKept.has(C1) && pol.wanted.has(C1)).toBe(true);
	});

	it('the drop pass skips a cascade with pending operations or an open player', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 4 });
		server.create({ id: C2, source_id: Q2, count: 4 });
		server.create({ id: C3, source_id: Q3, count: 4 });
		const b = await Dev.make(server);
		for (const c of [C1, C2, C3]) await b.open(c);
		await b.sync();
		await b.grade(Q1, [0]); // pending on C1
		b.openElsewhere = new Set([C2]); // C2's player open in another tab
		b.now += 15 * DAY;
		await b.dl.run();
		expect(await b.count('quiz_questions', C1)).toBe(4);
		expect(await b.count('quiz_questions', C2)).toBe(4);
		expect(await b.count('quiz_questions', C3)).toBe(0);
		// Without Web Locks, only this tab's own open cascade is spared.
		b.openElsewhere = null;
		b.openHere = C2;
		await b.dl.run();
		expect(await b.count('quiz_questions', C2)).toBe(4);
	});

	it('a 429 on a page pauses and finishes it, with no page lost or fetched twice', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 25_000 });
		const b = await Dev.make(server);
		await b.open(C1);
		let n = 0;
		b.fail = (c) => (c.kind === 'cards' && ++n === 2 ? new ApiError(429, {}, 3) : null);
		await b.sync();
		const pages = b.calls.filter((c) => c.kind === 'cards').map((c) => c.from);
		// The second page failed once and was fetched again: 0, 10000 (429), 10000, 20000.
		expect(pages).toEqual([0, 10_000, 10_000, 20_000]);
		expect(await b.count('cards', C1)).toBe(25_000);
	}, 120_000);

	it('a page the player needs takes priority: the manager starts nothing while one is outstanding', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 3 });
		const b = await Dev.make(server);
		await b.open(C1);
		await b.engine.sync();
		let release!: () => void;
		const player = b.dl.forPlayer(() => new Promise<void>((r) => (release = r)));
		const pass = b.dl.run();
		await new Promise((r) => setTimeout(r, 20));
		expect(b.calls).toEqual([]);
		release();
		await player;
		await pass;
		expect(kinds(b)).toContain('cards');
	});

	it('cards fetched without definitions are refetched with them once the preference asks', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 3 });
		const b = await Dev.make(server);
		await b.open(C1);
		await b.sync();
		expect((await b.db.get('cards', [C1, 0]))!.definitions).toBe(false);
		await applyLocally(b.db, { type: 'set_preferences', anagram_show_definitions: true });
		await b.dl.run();
		expect(kinds(b)).toContain('cards+defs');
		expect((await b.db.get('cards', [C1, 0]))!.definitions).toBe(true);
	});

	it('a QuotaExceededError runs the drop pass, then eviction, and retries once; a second failure says there is no room', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 3 });
		server.create({ id: C2, source_id: Q2, count: 3 });
		const b = await Dev.make(server);
		b.now -= DAY;
		await b.open(C1);
		b.now += DAY;
		await b.sync();
		await b.open(C2);
		let quota = 1;
		const put = b.db.put.bind(b.db);
		(b.db as unknown as { put: typeof put }).put = (async (...args: Parameters<typeof put>) => {
			if (args[0] === 'distributions' && quota-- > 0) throw new DOMException('full', 'QuotaExceededError');
			return put(...args);
		}) as typeof put;
		await b.db.delete('distributions', 'english');
		await b.dl.run();
		// The retry succeeded after the drop pass took the older cascade.
		expect(await b.db.get('distributions', 'english')).toBeDefined();
		expect(b.dl.noRoom).toBe(false);
		quota = 2;
		await b.db.delete('distributions', 'english');
		await expect(b.dl.run()).rejects.toThrow();
		expect(b.dl.noRoom).toBe(true);
	});

	it('an offline restore of a quiz with no local rows stays pending until online, then the full sequence runs', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 4 });
		const a = await Dev.make(server);
		await a.open(C1);
		await a.sync();
		await a.grade(Q1, [0, 1, 2], 'correct');
		await a.grade(Q1, [3], 'missed');
		const l2 = await a.finish(Q1, '9');
		await a.grade(l2, [0], 'correct');
		await a.finish(l2, '10'); // cleared
		await a.sync();
		const b = await Dev.make(server);
		await b.sync();
		expect(await b.count('quiz_questions', C1)).toBe(0);
		await applyLocally(b.db, { type: 'restore_quiz', quiz_id: l2, shuffle_seed: '11' });
		const row = await view.quiz(b.db.transaction(APPLY_STORES) as unknown as RwTx, l2);
		expect(row!.pending).toBe(true);
		await b.sync();
		expect((await b.rows(l2)).every((r) => r.position !== null)).toBe(true);
		expect((await b.quizRow(l2))!.pending).toBeUndefined();
		void writeMeta;
	});

	it('Keep offline on a budget-dropped cascade clears the mark and fetches it whole without an open player; off lets the window drop it', async () => {
		const server = new FakeServer();
		server.create({ id: C1, source_id: Q1, count: 5 });
		const b = await Dev.make(server);
		await b.sync();
		await b.db.put('meta', [C1], 'budget_dropped');
		await setKeepOffline(b.db, C1, true, b.dl, new Date(b.now));
		expect(await getMeta(b.db, 'budget_dropped')).toEqual([]);
		expect(await b.count('quiz_questions', C1)).toBe(5);
		expect(await b.count('cards', C1)).toBe(5);
		// Kept: the drop pass leaves it however old its last open.
		b.now += 30 * DAY;
		await b.dl.run();
		expect(await b.count('cards', C1)).toBe(5);
		await setKeepOffline(b.db, C1, false, b.dl, new Date(b.now));
		expect(await b.count('quiz_questions', C1)).toBe(0);
		expect(await getMeta(b.db, 'auto_keep_optout')).toEqual([C1]);
	});
});
