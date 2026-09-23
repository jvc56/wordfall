// Operations applied to the local stores (PLAN.md § Frontend → lib/local,
// § Offline and Sync → Operations). `applyOp` is the store-backed twin of
// `lib/cascade/sim.ts`: the same checks in the same order (PQ-013), writing the
// changed rows into the overlay. `applyLocally` runs it for a new operation,
// minting its `device_seq` and appending it to the outbox in the same
// IndexedDB transaction, so two tabs can never mint the same sequence; the
// rebase replays the outbox through `applyOp` alone.
import { questionsHash, shuffle } from '$lib/cascade/order';
import {
	checkAttempt,
	checkDeepest,
	checkLive,
	checkMoveCursor,
	checkQuizOptionFields,
	checkSegmentEnd,
	checkSegmentPastCursor,
	checkSegmentSize,
	finish,
	finishSegment,
	outcomeOf,
	Rejected,
	restoreQuiz,
	type CascadeState,
	type Grade,
	type NewQuiz,
	type Opts,
	type Progression,
	type QuizState
} from '$lib/cascade/rules';
import type { StoreName, UserDb } from './db';
import { readMeta, recordOpen, writeMeta } from './meta';
import type { AttemptRow, CascadeRow, OutboxEntry, QuestionRow, QuizRow, WireOp } from './rows';
import * as view from './view';
import type { RwTx } from './view';

/** The operation kinds that count as an open of their cascade (§ Downloads). */
export const OPENING = new Set(['grade', 'move_cursor', 'finish', 'finish_segment', 'restore_quiz']);

/** What an applied operation produced, as the server's result carries it. */
export interface LocalResult {
	cascade_id?: string;
	outcome?: string;
	new_quiz_question_count?: number;
	new_quiz_questions_hash?: string;
	completion?: { levels: number; attempts: number };
}

export interface ApplyCtx {
	device_id: string;
	/** The server's `now()` stand-in for `cleared_at`, `completed_at`, `trashed_at`. */
	now: string;
	max_quiz_questions: number;
}

/** The fields a new operation carries beyond id, device_seq, seen_seq and at. */
export type NewOp =
	| { type: 'grade'; quiz_id: string; attempt: number; attempt_seed: string; question_idx: number; grade: Grade }
	| { type: 'move_cursor'; quiz_id: string; attempt: number; attempt_seed: string; position: number }
	| { type: 'finish'; quiz_id: string; attempt: number; attempt_seed: string; shuffle_seed: string; new_quiz_id: string }
	| {
			type: 'finish_segment';
			quiz_id: string;
			attempt: number;
			attempt_seed: string;
			segment_end: number;
			shuffle_seed: string;
			new_quiz_id: string;
	  }
	| { type: 'restore_quiz'; quiz_id: string; shuffle_seed: string }
	| { type: 'trash_cascade' | 'restore_cascade' | 'purge_cascade'; cascade_id: string }
	| { type: 'purge_quiz'; quiz_id: string }
	| {
			type: 'set_cascade_options';
			cascade_id: string;
			segment_size?: number;
			progression?: Progression;
			require_alphabetical?: boolean;
	  }
	| {
			type: 'set_quiz_options';
			quiz_id: string;
			segment_size?: number;
			progression?: Progression;
			require_alphabetical?: boolean;
	  }
	| { type: 'set_preferences'; [field: string]: unknown }
	| { type: 'set_bindings'; bindings: unknown[] };

export const APPLY_STORES: StoreName[] = [...view.VIEW_STORES, 'meta', 'outbox'];

// ---------------------------------------------------------------------------
// Rows ↔ rule state
// ---------------------------------------------------------------------------

function optsOf(r: { segment_size: number; progression: Progression; require_alphabetical: boolean }): Opts {
	return { segment_size: r.segment_size, progression: r.progression, require_alphabetical: r.require_alphabetical };
}

function cascadeState(c: CascadeRow): CascadeState {
	return {
		clear_threshold: c.clear_threshold,
		opts: optsOf(c),
		depth: c.depth,
		peak_depth: c.peak_depth,
		attempts_since_completion: c.attempts_since_completion,
		trashed: c.trashed_at !== null
	};
}

function quizState(q: QuizRow): QuizState {
	return {
		level: q.level,
		origin: q.origin,
		segment_chain: q.segment_chain,
		opts: optsOf(q),
		attempt: q.attempt,
		seed: BigInt(q.shuffle_seed),
		question_count: q.question_count,
		cursor: q.cursor,
		run_start: q.run_start,
		active: q.status === 'active'
	};
}

const later = (a: string, b: string) => (Date.parse(a) >= Date.parse(b) ? a : b);

// ---------------------------------------------------------------------------
// Overlay writes
// ---------------------------------------------------------------------------

async function putCascade(tx: RwTx, c: CascadeRow, s?: CascadeState) {
	if (s) {
		c = { ...c, depth: s.depth, peak_depth: s.peak_depth, attempts_since_completion: s.attempts_since_completion };
	}
	await tx.objectStore('overlay_cascades').put(c);
}

async function putQuiz(tx: RwTx, q: QuizRow) {
	await tx.objectStore('overlay_quizzes').put(q);
}

/** Question rows with positions from the seed: `shuffle` gives the order. */
function rowsFor(quizId: string, cascadeId: string, questions: number[], seed: bigint): QuestionRow[] {
	const order = shuffle(questions, seed);
	const pos = new Map<number, number>();
	order.forEach((idx, p) => pos.set(idx, p));
	return questions.map((idx) => ({
		quiz_id: quizId,
		question_idx: idx,
		cascade_id: cascadeId,
		position: pos.get(idx)!,
		grade: null,
		graded_at: null,
		seq: 0
	}));
}

async function putQuestions(tx: RwTx, rows: QuestionRow[]) {
	const store = tx.objectStore('overlay_quiz_questions');
	await Promise.all(rows.map((r) => store.put(r)));
}

/**
 * A new attempt in place: attempt, seed, counters and cursor reset, and every
 * question row rewritten with its new position and no grade. A quiz with no
 * local rows is marked pending instead (a restore; § Downloads).
 */
async function resetQuiz(tx: RwTx, q: QuizRow, attempt: number, seed: bigint, at: string, ctx: ApplyCtx): Promise<QuizRow> {
	const rows = await view.questionsOf(tx, q.id);
	const reset: QuizRow = {
		...q,
		attempt,
		shuffle_seed: seed.toString(),
		correct_count: 0,
		missed_count: 0,
		cursor: 0,
		run_start: 0,
		cursor_moved_at: at,
		cursor_device_id: ctx.device_id
	};
	if (rows.length === q.question_count) {
		await putQuestions(
			tx,
			rowsFor(
				q.id,
				q.cascade_id,
				rows.map((r) => r.question_idx),
				seed
			)
		);
	} else {
		reset.pending = true;
	}
	return reset;
}

async function insertQuiz(
	tx: RwTx,
	id: string,
	parent: QuizRow,
	n: NewQuiz,
	at: string,
	ctx: ApplyCtx
): Promise<[number, string]> {
	const hash = questionsHash(n.questions).toString();
	await putQuiz(tx, {
		id,
		cascade_id: parent.cascade_id,
		level: n.level,
		origin: n.origin,
		origin_quiz_id: parent.id,
		origin_attempt: n.origin_attempt,
		origin_segment_end: n.origin_segment_end,
		status: 'active',
		segment_chain: n.segment_chain,
		...n.opts,
		options_changed_at: at,
		options_seq: 0,
		options_device_id: ctx.device_id,
		attempt: 1,
		shuffle_seed: n.seed.toString(),
		questions_hash: hash,
		question_count: n.questions.length,
		correct_count: 0,
		missed_count: 0,
		cursor: 0,
		cursor_moved_at: at,
		cursor_device_id: ctx.device_id,
		run_start: 0,
		created_at: ctx.now,
		created_seq: 0,
		last_activity_at: at,
		cleared_at: null,
		updated_seq: 0,
		seq: 0
	});
	await putQuestions(tx, rowsFor(id, parent.cascade_id, n.questions, n.seed));
	return [n.questions.length, hash];
}

/** A cascade's cleared quizzes get a full retention period from now. */
async function restartClocks(tx: RwTx, cascadeId: string, ctx: ApplyCtx) {
	for (const q of await view.quizzesOf(tx, cascadeId)) {
		if (q.status === 'cleared') await putQuiz(tx, { ...q, cleared_at: ctx.now });
	}
}

// ---------------------------------------------------------------------------
// Lookups with the server's rejections
// ---------------------------------------------------------------------------

async function loadQuiz(tx: RwTx, id: string): Promise<{ q: QuizRow; c: CascadeRow }> {
	const q = await view.quiz(tx, id);
	const c = q && (await view.cascade(tx, q.cascade_id));
	if (!q || !c) throw new Rejected('not_found');
	return { q, c };
}

async function loadCascade(tx: RwTx, id: string): Promise<CascadeRow> {
	const c = await view.cascade(tx, id);
	if (!c) throw new Rejected('not_found');
	return c;
}

async function idFree(tx: RwTx, id: string) {
	if (await view.quizIdTaken(tx, id)) throw new Rejected('invalid');
}

// ---------------------------------------------------------------------------
// The operations
// ---------------------------------------------------------------------------

/** Applies one operation to the overlay, or throws `Rejected` with the reason. */
export async function applyOp(tx: RwTx, op: WireOp, ctx: ApplyCtx): Promise<LocalResult> {
	const at = op.at;
	switch (op.type) {
		case 'grade': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			checkLive(cascadeState(c), quizState(q));
			checkAttempt(quizState(q), op.attempt as number, BigInt(op.attempt_seed as string));
			const row = await view.question(tx, q.id, op.question_idx as number);
			if (!row) throw new Rejected('not_found');
			const grade = op.grade as Grade;
			const d = (g: Grade | null, v: Grade) => (g === v ? 1 : 0);
			await tx.objectStore('overlay_quiz_questions').put({ ...row, grade, graded_at: at });
			await putQuiz(tx, {
				...q,
				correct_count: q.correct_count - d(row.grade, 'correct') + d(grade, 'correct'),
				missed_count: q.missed_count - d(row.grade, 'missed') + d(grade, 'missed'),
				last_activity_at: later(q.last_activity_at, at)
			});
			await putCascade(tx, { ...c, last_activity_at: later(c.last_activity_at, at) });
			return { cascade_id: c.id };
		}
		case 'move_cursor': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			const s = quizState(q);
			checkLive(cascadeState(c), s);
			checkAttempt(s, op.attempt as number, BigInt(op.attempt_seed as string));
			checkMoveCursor(s, op.position as number);
			await putQuiz(tx, { ...q, cursor: op.position as number, cursor_moved_at: at, cursor_device_id: ctx.device_id });
			return { cascade_id: c.id };
		}
		case 'finish': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			const cs = cascadeState(c);
			const qs = quizState(q);
			checkLive(cs, qs);
			// PQ-013: the attempt before the depth.
			checkAttempt(qs, op.attempt as number, BigInt(op.attempt_seed as string));
			checkDeepest(cs, qs);
			if (q.correct_count + q.missed_count !== q.question_count) throw new Rejected('ungraded');
			await idFree(tx, op.new_quiz_id as string);
			const rows = await view.questionsOf(tx, q.id);
			const misses = rows.filter((r) => r.grade === 'missed').map((r) => r.question_idx);
			const seed = BigInt(op.shuffle_seed as string);
			const r = finish(cs, qs, q.correct_count, misses, seed);
			const outcome = outcomeOf(r);
			const attempt: AttemptRow = {
				quiz_id: q.id,
				attempt: q.attempt,
				cascade_id: c.id,
				question_count: q.question_count,
				correct_count: q.correct_count,
				missed_count: q.missed_count,
				outcome,
				shuffle_seed: q.shuffle_seed,
				finished_at: at,
				updated_seq: 0,
				seq: 0
			};
			await tx.objectStore('overlay_quiz_attempts').put(attempt);
			const out: LocalResult = { cascade_id: c.id, outcome };
			let cascadeRow: CascadeRow = { ...c, last_activity_at: later(c.last_activity_at, at) };
			let quizRow: QuizRow = { ...q, last_activity_at: later(q.last_activity_at, at) };
			if (r.kind === 'finished') {
				quizRow = { ...quizRow, status: 'cleared', cleared_at: ctx.now };
				if (r.replacement) {
					[out.new_quiz_question_count, out.new_quiz_questions_hash] = await insertQuiz(
						tx,
						op.new_quiz_id as string,
						q,
						r.replacement,
						at,
						ctx
					);
				}
			} else {
				quizRow = await resetQuiz(tx, quizRow, q.attempt + 1, r.reset_seed, at, ctx);
				if (r.kind === 'descended') {
					[out.new_quiz_question_count, out.new_quiz_questions_hash] = await insertQuiz(
						tx,
						op.new_quiz_id as string,
						q,
						r.new_level,
						at,
						ctx
					);
				} else {
					out.new_quiz_question_count = q.question_count;
					out.new_quiz_questions_hash = q.questions_hash;
					if (r.kind === 'completed') {
						out.completion = { levels: r.levels, attempts: r.attempts };
						cascadeRow = { ...cascadeRow, completed_at: ctx.now };
					}
				}
			}
			await putQuiz(tx, quizRow);
			await putCascade(tx, cascadeRow, cs);
			return out;
		}
		case 'finish_segment': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			const cs = cascadeState(c);
			const qs = quizState(q);
			const end = op.segment_end as number;
			checkLive(cs, qs);
			// PQ-013: the attempt and the duplicate before the depth.
			checkAttempt(qs, op.attempt as number, BigInt(op.attempt_seed as string));
			checkSegmentEnd(qs, end);
			for (const o of await view.quizzesOf(tx, c.id)) {
				if (o.origin === 'segment' && o.origin_quiz_id === q.id && o.origin_attempt === q.attempt && o.origin_segment_end === end) {
					throw new Rejected('duplicate_segment');
				}
			}
			checkDeepest(cs, qs);
			checkSegmentPastCursor(qs, end);
			const rows = await view.questionsOf(tx, q.id);
			const run = rows.filter((r) => r.position !== null && r.position < end);
			if (run.length !== end || run.some((r) => r.grade === null)) throw new Rejected('ungraded');
			await idFree(tx, op.new_quiz_id as string);
			const runMisses = run
				.filter((r) => r.position! >= q.run_start && r.grade === 'missed')
				.map((r) => r.question_idx)
				.sort((a, b) => a - b);
			const r = finishSegment(cs, qs, runMisses, end, BigInt(op.shuffle_seed as string));
			const out: LocalResult = { cascade_id: c.id, outcome: r.kind };
			if (r.kind === 'drilled') {
				[out.new_quiz_question_count, out.new_quiz_questions_hash] = await insertQuiz(
					tx,
					op.new_quiz_id as string,
					q,
					r.new_level,
					at,
					ctx
				);
			}
			await putQuiz(tx, {
				...q,
				cursor: end,
				run_start: end,
				cursor_moved_at: at,
				cursor_device_id: ctx.device_id,
				last_activity_at: later(q.last_activity_at, at)
			});
			await putCascade(tx, { ...c, last_activity_at: later(c.last_activity_at, at) }, cs);
			return out;
		}
		case 'restore_quiz': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			if (q.status === 'active') throw new Rejected('not_cleared');
			const cs = cascadeState(c);
			const r = restoreQuiz(cs, quizState(q), BigInt(op.shuffle_seed as string));
			let row = await resetQuiz(tx, q, r.attempt, r.seed, at, ctx);
			row = {
				...row,
				status: 'active',
				cleared_at: null,
				level: r.level,
				...r.opts,
				options_changed_at: at,
				options_device_id: ctx.device_id,
				last_activity_at: later(q.last_activity_at, at)
			};
			await putQuiz(tx, row);
			await putCascade(
				tx,
				{
					...c,
					trashed_at: cs.trashed ? c.trashed_at : null,
					last_activity_at: later(c.last_activity_at, at)
				},
				cs
			);
			// Brought back with its quiz, and only then its cleared quizzes' clocks restart.
			if (r.cascade_restored) await restartClocks(tx, c.id, ctx);
			return {
				cascade_id: c.id,
				new_quiz_question_count: q.question_count,
				new_quiz_questions_hash: q.questions_hash
			};
		}
		case 'trash_cascade': {
			const c = await loadCascade(tx, op.cascade_id as string);
			if (c.trashed_at !== null) throw new Rejected('trashed');
			await putCascade(tx, { ...c, trashed_at: ctx.now });
			return { cascade_id: c.id };
		}
		case 'restore_cascade': {
			const c = await loadCascade(tx, op.cascade_id as string);
			if (c.trashed_at === null) throw new Rejected('not_trashed');
			await putCascade(tx, { ...c, trashed_at: null, last_activity_at: later(c.last_activity_at, at) });
			await restartClocks(tx, c.id, ctx);
			return { cascade_id: c.id };
		}
		case 'purge_quiz': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			if (c.trashed_at !== null) throw new Rejected('trashed');
			if (q.status === 'active') throw new Rejected('not_cleared');
			await putQuiz(tx, { ...q, deleted: true });
			return { cascade_id: c.id };
		}
		case 'purge_cascade': {
			const c = await loadCascade(tx, op.cascade_id as string);
			if (c.trashed_at === null) throw new Rejected('not_trashed');
			for (const q of await view.quizzesOf(tx, c.id)) await putQuiz(tx, { ...q, deleted: true });
			await putCascade(tx, { ...c, deleted: true });
			return { cascade_id: c.id };
		}
		case 'set_cascade_options': {
			const c = await loadCascade(tx, op.cascade_id as string);
			if (c.trashed_at !== null) throw new Rejected('trashed');
			const next = { ...c, options_changed_at: at, options_device_id: ctx.device_id };
			if (op.segment_size !== undefined) next.segment_size = checkSegmentSize(op.segment_size as number, ctx.max_quiz_questions);
			if (op.progression !== undefined) next.progression = op.progression as Progression;
			if (op.require_alphabetical !== undefined) next.require_alphabetical = op.require_alphabetical as boolean;
			await putCascade(tx, next);
			return { cascade_id: c.id };
		}
		case 'set_quiz_options': {
			const { q, c } = await loadQuiz(tx, op.quiz_id as string);
			const qs = quizState(q);
			checkLive(cascadeState(c), qs);
			const size =
				op.segment_size === undefined ? undefined : checkSegmentSize(op.segment_size as number, ctx.max_quiz_questions);
			checkQuizOptionFields(qs, op.progression !== undefined, op.segment_size !== undefined);
			const next = { ...q, options_changed_at: at, options_device_id: ctx.device_id };
			if (size !== undefined) next.segment_size = size;
			if (op.progression !== undefined) next.progression = op.progression as Progression;
			if (op.require_alphabetical !== undefined) next.require_alphabetical = op.require_alphabetical as boolean;
			await putQuiz(tx, next);
			return { cascade_id: c.id };
		}
		case 'set_preferences':
		case 'set_bindings':
			// No overlay rows: the preferences view is the base row with the
			// pending operations' fields laid over it (see `preferencesView`).
			return {};
		default:
			throw new Rejected('invalid');
	}
}

// ---------------------------------------------------------------------------
// applyLocally
// ---------------------------------------------------------------------------

export interface Applied {
	entry: OutboxEntry;
	result: LocalResult;
}

/**
 * Applies a new operation on this device: its rows into the overlay, its
 * `device_seq` read and advanced in `meta`, and the operation appended to the
 * outbox, all in one IndexedDB transaction. A rejection aborts the
 * transaction and is rethrown; so is a `QuotaExceededError`, and then nothing
 * reached the overlay or the outbox (§ On the device, the quota path).
 */
export async function applyLocally(db: UserDb, fields: NewOp, clock: () => Date = () => new Date()): Promise<Applied> {
	const tx = db.transaction(APPLY_STORES, 'readwrite') as unknown as RwTx;
	// An aborted transaction's `done` rejects; the error that caused it is what is thrown.
	tx.done.catch(() => undefined);
	try {
		const device = await readMeta(tx, 'device');
		const sync = await readMeta(tx, 'sync');
		const server = await readMeta(tx, 'server');
		const now = clock().toISOString();
		const op: WireOp = {
			...fields,
			id: crypto.randomUUID(),
			device_seq: device.next_device_seq,
			seen_seq: sync.cursor ?? 0,
			at: now
		};
		const result = await applyOp(tx, op, { device_id: device.device_id, now, max_quiz_questions: server.max_quiz_questions });
		const entry: OutboxEntry = { device_seq: op.device_seq, op };
		if (result.cascade_id) entry.cascade_id = result.cascade_id;
		const outbox = tx.objectStore('outbox');
		if (op.type === 'move_cursor') {
			// Only the latest pending cursor move per quiz: the old one goes and the
			// new one is appended with a fresh device_seq, never written into the old slot.
			const quizId = op.quiz_id as string;
			for (const key of await outbox.index('cursor_quiz').getAllKeys(quizId)) await outbox.delete(key);
			entry.cursor_quiz = quizId;
		}
		await outbox.add(entry);
		await writeMeta(tx, 'device', { ...device, next_device_seq: device.next_device_seq + 1 });
		if (result.cascade_id && OPENING.has(op.type)) await recordOpen(tx, result.cascade_id, now);
		await tx.done;
		return { entry, result };
	} catch (e) {
		try {
			tx.abort();
		} catch {
			// Already finished or aborted.
		}
		throw e;
	}
}

/** The operations waiting to be sent, in `device_seq` order. */
export async function pendingOps(db: UserDb): Promise<OutboxEntry[]> {
	return db.getAll('outbox');
}

/** Whether a cascade holds pending operations (nothing is dropped from it then). */
export async function hasPending(db: UserDb, cascadeId: string): Promise<boolean> {
	return (await db.countFromIndex('outbox', 'cascade_id', cascadeId)) > 0;
}

