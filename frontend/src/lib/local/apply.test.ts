// PLAN.md § Unit tests → Frontend: "The lib/local tests include two writers in
// one fake-indexeddb never minting the same device_seq, a write from a second
// tab during a rebase landing on the rebuilt overlay, and a fixture database
// at each earlier schema version, holding an outbox and an overlay, opening at
// the current version with every row intact"; the outbox keeping one
// move_cursor per quiz; and applyLocally held to the shared rule vectors
// (§ Contract fixtures), since it applies the same rules to the stores.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { openDB } from 'idb';
import { beforeEach, describe, expect, it } from 'vitest';
import rules from '../../../../contract-fixtures/cascade/rules.json';
import { questionsHash, shuffle } from '$lib/cascade/order';
import { nextBoundary, Rejected, runIndicator, runIndicatorText, type Progression, type QuizState } from '$lib/cascade/rules';
import { userDbName } from './accounts';
import { applyLocally, APPLY_STORES, type NewOp } from './apply';
import { MIGRATIONS, openUserDb, USER_DB_VERSION, type UserDb } from './db';
import { getMeta, initMeta } from './meta';
import { defaultPreferences, layPreferences, preferencesView } from './preferences';
import type { CascadeRow, QuestionRow, QuizRow } from './rows';
import * as view from './view';
import type { RwTx } from './view';

type Json = Record<string, unknown>;
const USER = '11111111-1111-4111-8111-111111111111';

beforeEach(() => {
	globalThis.indexedDB = new IDBFactory();
});

async function freshDb(): Promise<UserDb> {
	const db = await openUserDb(USER);
	await initMeta(db, USER, 'alice');
	return db;
}

interface Setup {
	cascade_id: string;
	source_id: string;
	count: number;
	threshold?: number;
	segment_size?: number;
	progression?: Progression;
	require_alphabetical?: boolean;
	seed: bigint;
}

/** A cascade and its Source quiz in the base, as a first pull leaves them. */
async function seed(db: UserDb, s: Setup) {
	const t = '2026-01-01T00:00:00.000Z';
	const opts = {
		segment_size: s.segment_size ?? 0,
		progression: s.progression ?? ('ladder' as Progression),
		require_alphabetical: s.require_alphabetical ?? false
	};
	const c: CascadeRow = {
		id: s.cascade_id,
		name: 'test',
		quiz_type: 'anagram',
		lexicon: 'EN-FIX',
		letter_distribution: 'english',
		clear_threshold: s.threshold ?? 80,
		...opts,
		options_changed_at: t,
		options_seq: 1,
		options_device_id: 'd',
		question_count: s.count,
		depth: 1,
		peak_depth: 1,
		attempts_since_completion: 0,
		created_at: t,
		last_activity_at: t,
		completed_at: null,
		trashed_at: null,
		updated_seq: 1,
		seq: 1
	};
	const idx = Array.from({ length: s.count }, (_, i) => i);
	const q: QuizRow = {
		id: s.source_id,
		cascade_id: s.cascade_id,
		level: 1,
		origin: 'source',
		origin_quiz_id: null,
		origin_attempt: null,
		origin_segment_end: null,
		status: 'active',
		segment_chain: false,
		...opts,
		options_changed_at: t,
		options_seq: 1,
		options_device_id: 'd',
		attempt: 1,
		shuffle_seed: s.seed.toString(),
		questions_hash: questionsHash(idx).toString(),
		question_count: s.count,
		correct_count: 0,
		missed_count: 0,
		cursor: 0,
		cursor_moved_at: null,
		cursor_device_id: null,
		run_start: 0,
		created_at: t,
		created_seq: 1,
		last_activity_at: t,
		cleared_at: null,
		updated_seq: 1,
		seq: 1
	};
	const tx = db.transaction(['cascades', 'quizzes', 'quiz_questions'], 'readwrite');
	await tx.objectStore('cascades').put(c);
	await tx.objectStore('quizzes').put(q);
	const order = shuffle(idx, s.seed);
	const store = tx.objectStore('quiz_questions');
	await Promise.all(
		order.map((i, p) =>
			store.put({ quiz_id: q.id, question_idx: i, cascade_id: c.id, position: p, grade: null, graded_at: null, seq: 1 })
		)
	);
	await tx.done;
}

function quizState(q: QuizRow): QuizState {
	return {
		level: q.level,
		origin: q.origin,
		segment_chain: q.segment_chain,
		opts: { segment_size: q.segment_size, progression: q.progression, require_alphabetical: q.require_alphabetical },
		attempt: q.attempt,
		seed: BigInt(q.shuffle_seed),
		question_count: q.question_count,
		cursor: q.cursor,
		run_start: q.run_start,
		active: q.status === 'active'
	};
}

/** The vector's state as the local view shows it. */
async function snapshot(db: UserDb, cascadeId: string, completions: number) {
	const tx = db.transaction(APPLY_STORES) as unknown as RwTx;
	const c = await view.cascade(tx, cascadeId);
	const quizzes = [];
	for (const q of (await view.quizzesOf(tx, cascadeId)).sort((a, b) => (a.id < b.id ? -1 : 1))) {
		const rows = await view.questionsOf(tx, q.id);
		const s = quizState(q);
		const ri = s.active ? runIndicator(s) : null;
		const questions = rows.map((r) => r.question_idx);
		// Invariants: counters match grades; positions come from the seed.
		expect(q.correct_count).toBe(rows.filter((r) => r.grade === 'correct').length);
		expect(q.missed_count).toBe(rows.filter((r) => r.grade === 'missed').length);
		const byPos = [...rows].sort((a, b) => a.position! - b.position!).map((r) => r.question_idx);
		expect(byPos).toEqual(shuffle(questions, s.seed));
		quizzes.push({
			id: q.id,
			level: q.level,
			status: q.status,
			origin: q.origin,
			origin_quiz_id: q.origin_quiz_id,
			origin_attempt: q.origin_attempt,
			origin_segment_end: q.origin_segment_end,
			segment_chain: q.segment_chain,
			segment_size: q.segment_size,
			progression: q.progression,
			require_alphabetical: q.require_alphabetical,
			attempt: q.attempt,
			shuffle_seed: q.shuffle_seed,
			question_count: q.question_count,
			questions_hash: q.questions_hash,
			questions,
			correct: q.correct_count,
			missed: q.missed_count,
			cursor: q.cursor,
			run_start: q.run_start,
			next_boundary: s.active ? nextBoundary(s) : null,
			run_indicator: ri ? runIndicatorText(ri) : null
		});
	}
	const cascade = c
		? {
				depth: c.depth,
				peak_depth: c.peak_depth,
				attempts_since_completion: c.attempts_since_completion,
				completions,
				trashed: c.trashed_at !== null,
				purged: false,
				segment_size: c.segment_size,
				progression: c.progression,
				require_alphabetical: c.require_alphabetical
			}
		: null;
	return { cascade, quizzes };
}

function newOp(o: Json, cascadeId: string): NewOp {
	const { type, quiz, ...rest } = o;
	const fields: Json = { type, ...rest };
	if (quiz !== undefined) fields.quiz_id = quiz;
	if (['trash_cascade', 'restore_cascade', 'purge_cascade', 'set_cascade_options'].includes(type as string)) {
		fields.cascade_id = cascadeId;
	}
	return fields as unknown as NewOp;
}

describe('applyLocally against the rule vectors', () => {
	for (const v of rules.vectors as unknown as Json[]) {
		it(v.name as string, async () => {
			const db = await freshDb();
			const s = v.setup as Json;
			const cascadeId = 'c0000000-0000-4000-8000-000000000000';
			await seed(db, {
				cascade_id: cascadeId,
				source_id: s.source_quiz_id as string,
				count: s.question_count as number,
				threshold: s.clear_threshold as number,
				segment_size: s.segment_size as number,
				progression: s.progression as Progression,
				require_alphabetical: s.require_alphabetical as boolean,
				seed: BigInt(s.source_seed as string)
			});
			let completions = 0;
			const steps = v.steps as Json[];
			for (let i = 0; i < steps.length; i++) {
				const step = steps[i];
				const want = { ...(step.result as Json) };
				delete want.attempt;
				let got: Json;
				try {
					const { result } = await applyLocally(db, newOp(step.op as Json, cascadeId));
					got = { status: 'applied' };
					if (result.outcome !== undefined) got.outcome = result.outcome;
					if (result.new_quiz_question_count !== undefined) got.new_quiz_question_count = result.new_quiz_question_count;
					if (result.new_quiz_questions_hash !== undefined) got.new_quiz_questions_hash = result.new_quiz_questions_hash;
					if (result.completion) {
						got.completion = result.completion;
						completions++;
					}
				} catch (e) {
					if (!(e instanceof Rejected)) throw e;
					got = { status: 'rejected', reason: e.reason };
				}
				expect(got, `${v.name} step ${i}`).toEqual(want);
				if (step.state) {
					const st = step.state as { cascade: Json; quizzes: Json[] };
					const snap = await snapshot(db, cascadeId, completions);
					if (st.cascade.purged) {
						expect(snap.cascade, `${v.name} step ${i}`).toBeNull();
						expect(snap.quizzes).toEqual([]);
					} else {
						expect(snap, `${v.name} step ${i} state`).toEqual(st);
					}
				}
			}
		}, 60_000);
	}
});

describe('the outbox', () => {
	const setup: Setup = {
		cascade_id: 'c1',
		source_id: 'q1',
		count: 10,
		segment_size: 5,
		seed: 0x8000000000001234n
	};

	async function grade(db: UserDb, position: number, g: 'correct' | 'missed' = 'correct') {
		const tx = db.transaction(APPLY_STORES) as unknown as RwTx;
		const q = (await view.quiz(tx, 'q1'))!;
		const row = (await view.questionsOf(tx, 'q1')).find((r) => r.position === position)!;
		return applyLocally(db, {
			type: 'grade',
			quiz_id: 'q1',
			attempt: q.attempt,
			attempt_seed: q.shuffle_seed,
			question_idx: row.question_idx,
			grade: g
		});
	}

	async function move(db: UserDb, position: number) {
		const q = (await db.get('overlay_quizzes', 'q1')) ?? (await db.get('quizzes', 'q1'))!;
		return applyLocally(db, {
			type: 'move_cursor',
			quiz_id: 'q1',
			attempt: q.attempt,
			attempt_seed: q.shuffle_seed,
			position
		});
	}

	it('two writers never mint the same device_seq', async () => {
		const a = await freshDb();
		await seed(a, setup);
		const b = await openUserDb(USER);
		const ops = Array.from({ length: 20 }, (_, i) => grade(i % 2 ? a : b, i % 10));
		await Promise.all(ops);
		const seqs = (await a.getAll('outbox')).map((e) => e.device_seq);
		expect(seqs).toEqual(Array.from({ length: 20 }, (_, i) => i + 1));
		expect((await getMeta(a, 'device')).next_device_seq).toBe(21);
	});

	it('keeps one move_cursor per quiz, re-appended after a queued finish_segment', async () => {
		const db = await freshDb();
		await seed(db, setup);
		for (let p = 0; p < 4; p++) {
			await grade(db, p);
			await move(db, p + 1);
		}
		await grade(db, 4, 'missed');
		let cursorMoves = (await db.getAll('outbox')).filter((e) => e.op.type === 'move_cursor');
		expect(cursorMoves).toHaveLength(1);
		expect(cursorMoves[0].op.position).toBe(4);
		const q = (await db.get('overlay_quizzes', 'q1'))!;
		const fs = await applyLocally(db, {
			type: 'finish_segment',
			quiz_id: 'q1',
			attempt: 1,
			attempt_seed: q.shuffle_seed,
			segment_end: 5,
			shuffle_seed: '77',
			new_quiz_id: 'q2'
		});
		expect(fs.result.outcome).toBe('drilled');
		const after = await move(db, 6);
		cursorMoves = (await db.getAll('outbox')).filter((e) => e.op.type === 'move_cursor');
		expect(cursorMoves).toHaveLength(1);
		expect(cursorMoves[0].device_seq).toBe(after.entry.device_seq);
		expect(after.entry.device_seq).toBeGreaterThan(fs.entry.device_seq);
		// device_seq has gaps; only the order matters.
		const seqs = (await db.getAll('outbox')).map((e) => e.device_seq);
		expect(seqs).toEqual([...seqs].sort((x, y) => x - y));
	});

	it('a rejected operation writes nothing and mints no sequence', async () => {
		const db = await freshDb();
		await seed(db, setup);
		await expect(
			applyLocally(db, { type: 'grade', quiz_id: 'q1', attempt: 1, attempt_seed: '5', question_idx: 0, grade: 'correct' })
		).rejects.toMatchObject({ reason: 'stale_attempt' });
		expect(await db.count('outbox')).toBe(0);
		expect(await db.count('overlay_quiz_questions')).toBe(0);
		expect((await getMeta(db, 'device')).next_device_seq).toBe(1);
	});

	it('records an open for studying, never for trash or options', async () => {
		const db = await freshDb();
		await seed(db, setup);
		await applyLocally(db, { type: 'set_cascade_options', cascade_id: 'c1', segment_size: 0 });
		await applyLocally(db, { type: 'trash_cascade', cascade_id: 'c1' });
		expect(await getMeta(db, 'opens')).toEqual({});
		await applyLocally(db, { type: 'restore_cascade', cascade_id: 'c1' });
		await grade(db, 0);
		expect(Object.keys(await getMeta(db, 'opens'))).toEqual(['c1']);
	});

	it('stamps seen_seq with the sync cursor, or 0 before the first sync', async () => {
		const db = await freshDb();
		await seed(db, setup);
		expect((await grade(db, 0)).entry.op.seen_seq).toBe(0);
		await db.put('meta', { cursor: 42, last_sync_at: null }, 'sync');
		expect((await grade(db, 1)).entry.op.seen_seq).toBe(42);
	});

	it("a write from a second tab during a rebase lands on the rebuilt overlay", async () => {
		const a = await freshDb();
		await seed(a, setup);
		await grade(a, 0);
		const b = await openUserDb(USER);
		// Tab A's rebase: one transaction over the outbox and the overlay that
		// clears the overlay and rebuilds it (here, the grade now in the base).
		const rebase = (async () => {
			const tx = a.transaction(APPLY_STORES, 'readwrite');
			const row = (await tx.objectStore('overlay_quiz_questions').getAll())[0];
			const q = (await tx.objectStore('overlay_quizzes').get('q1'))!;
			await tx.objectStore('overlay_quiz_questions').clear();
			await tx.objectStore('overlay_quizzes').clear();
			await tx.objectStore('overlay_cascades').clear();
			await tx.objectStore('outbox').clear();
			await tx.objectStore('quiz_questions').put({ ...row, seq: 2 });
			await tx.objectStore('quizzes').put({ ...q, seq: 2 });
			await tx.done;
		})();
		// Tab B grades the next card while the rebase runs.
		const write = grade(b, 1, 'missed');
		await Promise.all([rebase, write]);
		const q = (await b.get('overlay_quizzes', 'q1'))!;
		expect([q.correct_count, q.missed_count]).toEqual([1, 1]);
		expect(await b.count('outbox')).toBe(1);
		expect(await b.count('overlay_quiz_questions')).toBe(1);
	});
});

describe('the per-user database', () => {
	it('opens a fixture database of each earlier schema version at the current one with every row intact', async () => {
		for (let v = 1; v <= USER_DB_VERSION; v++) {
			globalThis.indexedDB = new IDBFactory();
			const name = userDbName(USER);
			// The fixture: the schema as of version v, holding an outbox and an overlay.
			const old = await openDB(name, v, {
				upgrade(db, oldVersion) {
					for (let i = oldVersion; i < v; i++) MIGRATIONS[i](db as never);
				}
			});
			const q = { id: 'q1', cascade_id: 'c1', status: 'active' } as unknown as QuizRow;
			const row = { quiz_id: 'q1', question_idx: 3, cascade_id: 'c1', position: 0, grade: 'missed', graded_at: 't', seq: 0 };
			const entry = { device_seq: 7, cascade_id: 'c1', op: { id: 'o', device_seq: 7, seen_seq: 0, at: 't', type: 'grade' } };
			await old.put('overlay_quizzes', q);
			await old.put('overlay_quiz_questions', row as QuestionRow);
			await old.put('outbox', entry);
			old.close();
			const db = await openUserDb(USER);
			expect(db.version).toBe(USER_DB_VERSION);
			expect(await db.get('overlay_quizzes', 'q1')).toEqual(q);
			expect(await db.get('overlay_quiz_questions', ['q1', 3])).toEqual(row);
			expect(await db.getAll('outbox')).toEqual([entry]);
			db.close();
		}
	});

	it('closes on versionchange so a newer build is never blocked', async () => {
		let told = false;
		const db = await openUserDb(USER, () => {
			told = true;
		});
		const newer = await openDB(userDbName(USER), USER_DB_VERSION + 1);
		expect(newer.version).toBe(USER_DB_VERSION + 1);
		expect(told).toBe(true);
		newer.close();
		void db;
	});

	it('makes the device id once per device and user', async () => {
		const db = await freshDb();
		const first = await getMeta(db, 'device');
		await initMeta(db, USER, 'alice');
		expect(await getMeta(db, 'device')).toEqual(first);
		expect(first.next_device_seq).toBe(1);
	});
});

describe('the preferences view', () => {
	it('uses the documented defaults before the first pull and lays pending changes over the base', async () => {
		const db = await freshDb();
		expect((await preferencesView(db)).default_clear_threshold).toBe(80);
		await applyLocally(db, { type: 'set_preferences', default_clear_threshold: 70 });
		await applyLocally(db, { type: 'set_preferences', leave_value_decimals: 2, default_clear_threshold: 75 });
		await applyLocally(db, {
			type: 'set_bindings',
			bindings: [{ action: 'show_next', kind: 'key', code: 'Enter', ctrl: false, shift: false, alt: false, meta: false }]
		});
		const v = await preferencesView(db);
		expect([v.default_clear_threshold, v.leave_value_decimals]).toEqual([75, 2]);
		expect(v.bindings.map((b) => b.code)).toEqual(['Enter']);
		// Dropping the pending operation (a stale rejection) reverts the view.
		expect(layPreferences(defaultPreferences(), []).default_clear_threshold).toBe(80);
	});
});
