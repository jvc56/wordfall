// Applying a pull on the device, once its last page has arrived (PLAN.md
// § The sync cycle → Applying a pull on the device, steps 1–5 and the fast
// path, and § The sync cycle → Pull, 3, for a full pull's deletions).
// Steps 2 to 4 run in one IndexedDB transaction per cascade that includes the
// outbox, so the player never observes an empty overlay and an `applyLocally`
// from another tab waits for the rebase and lands on the rebuilt overlay.
import { questionsHash, shuffle } from '$lib/cascade/order';
import { Rejected } from '$lib/cascade/rules';
import { applyOp } from '$lib/local/apply';
import { BASE, OVERLAY, STAGING, type StoreName, type UserDb } from '$lib/local/db';
import { readMeta, writeMeta } from '$lib/local/meta';
import type { OutboxEntry, QuestionRow, QuizRow } from '$lib/local/rows';
import type { RwTx } from '$lib/local/view';
import { noticesFor, type Notice, type Settled } from './notices';
import type { SyncResult } from './protocol';
import { mergeQuiz, type PullState } from './pull';

export interface Acked {
	entry: OutboxEntry;
	result: SyncResult;
}

export interface RebaseOutcome {
	notices: Notice[];
	/** Cascades whose rows changed: the player refreshes if it shows one (step 5). */
	changed: Set<string>;
	/** Active quizzes whose index list must be fetched before they can be played. */
	needsIndexList: Set<string>;
}

const REBASE_STORES: StoreName[] = [...BASE, ...STAGING, ...OVERLAY, 'outbox', 'meta', 'questions', 'cards'];

/** Operations that create, reset or restore a quiz: never on the fast path. */
const CREATING = new Set(['finish', 'finish_segment', 'restore_quiz']);

// ---------------------------------------------------------------------------
// Deletions
// ---------------------------------------------------------------------------

async function deleteQuizRows(tx: RwTx, quizId: string) {
	for (const store of ['quiz_questions', 'quiz_attempts'] as const) {
		const s = tx.objectStore(store);
		for (const k of await s.index('quiz_id').getAllKeys(quizId)) await s.delete(k);
	}
}

/**
 * Removes a cascade from this device: its base rows, its `questions` and
 * `cards` entries, and its `meta` entries (last-open time, Keep offline id,
 * automatic-keep opt-out, budget-dropped mark), as a tombstone does.
 */
export async function deleteCascadeHere(tx: RwTx, cascadeId: string) {
	await tx.objectStore('cascades').delete(cascadeId);
	for (const store of ['quizzes', 'quiz_questions', 'quiz_attempts', 'questions', 'cards'] as const) {
		const s = tx.objectStore(store);
		for (const k of await s.index('cascade_id').getAllKeys(cascadeId)) await s.delete(k as never);
	}
	const opens = await readMeta(tx, 'opens');
	if (cascadeId in opens) {
		delete opens[cascadeId];
		await writeMeta(tx, 'opens', opens);
	}
	for (const key of ['keep_offline', 'auto_keep_optout', 'budget_dropped'] as const) {
		const ids = await readMeta(tx, key);
		if (ids.includes(cascadeId)) {
			await writeMeta(
				tx,
				key,
				ids.filter((id) => id !== cascadeId)
			);
		}
	}
}

async function clearOverlay(tx: RwTx, cascadeId: string, keep?: (store: string, row: unknown) => boolean) {
	const oc = tx.objectStore('overlay_cascades');
	const row = await oc.get(cascadeId);
	if (row && !keep?.('cascades', row)) await oc.delete(cascadeId);
	for (const store of ['overlay_quizzes', 'overlay_quiz_questions', 'overlay_quiz_attempts'] as const) {
		const s = tx.objectStore(store);
		let cursor = await s.index('cascade_id').openCursor(cascadeId);
		while (cursor) {
			if (!keep?.(store, cursor.value)) await cursor.delete();
			cursor = await cursor.continue();
		}
	}
}

// ---------------------------------------------------------------------------
// Question rows
// ---------------------------------------------------------------------------

/** Positions for every row from the seed; grades untouched (a materialisation). */
function withPositions(rows: QuestionRow[], seed: string): QuestionRow[] {
	const idx = rows.map((r) => r.question_idx).sort((a, b) => a - b);
	const pos = new Map<number, number>();
	shuffle(idx, BigInt(seed)).forEach((i, p) => pos.set(i, p));
	return rows.map((r) => ({ ...r, position: pos.get(r.question_idx)! }));
}

// ---------------------------------------------------------------------------
// The rebase
// ---------------------------------------------------------------------------

interface CascadeWork {
	acked: Acked[];
	/** Quizzes an applied operation of this batch created (id → its op). */
	created: Map<string, Acked>;
	/** Quizzes an applied operation reset or restored. */
	reset: Set<string>;
}

function workFor(acked: Acked[]): CascadeWork {
	const w: CascadeWork = { acked, created: new Map(), reset: new Set() };
	for (const a of acked) {
		if (a.result.status !== 'applied') continue;
		const op = a.entry.op;
		const out = a.result.outcome;
		if (op.type === 'finish') {
			if (out === 'descended' || out === 'completed' || out === 'reshuffled') w.reset.add(op.quiz_id as string);
			if ((out === 'descended' || out === 'cleared' || out === 'replaced') && a.result.new_quiz_question_count !== undefined) {
				w.created.set(op.new_quiz_id as string, a);
			}
		} else if (op.type === 'finish_segment' && out === 'drilled') {
			w.created.set(op.new_quiz_id as string, a);
		} else if (op.type === 'restore_quiz') {
			w.reset.add(op.quiz_id as string);
		}
	}
	return w;
}

export interface RebaseCtx {
	device_id: string;
	max_quiz_questions: number;
	now: () => Date;
	/** False for a response that carried no pull (`resync_required`, `426`): the cursor stays. */
	advance?: boolean;
}

/**
 * Steps 1–5 for everything the pull and the acknowledged operations touched.
 * The cursor advances only once every cascade's transaction has committed, so
 * an interrupted rebase leaves it unadvanced and the next pull converges.
 */
export async function rebase(db: UserDb, st: PullState, acked: Acked[], ctx: RebaseCtx): Promise<RebaseOutcome> {
	const out: RebaseOutcome = { notices: [], changed: new Set(), needsIndexList: new Set() };

	// Step 1: drop every acknowledged operation, applied or rejected.
	{
		const tx = db.transaction(['outbox'], 'readwrite');
		await Promise.all(acked.map((a) => tx.objectStore('outbox').delete(a.entry.device_seq)));
		await tx.done;
	}

	const byCascade = new Map<string, Acked[]>();
	for (const a of acked) {
		if (!a.entry.cascade_id) continue;
		const list = byCascade.get(a.entry.cascade_id) ?? [];
		list.push(a);
		byCascade.set(a.entry.cascade_id, list);
	}
	const cascades = new Set<string>([...st.touched, ...byCascade.keys()]);
	if (st.full) for (const id of await db.getAllKeys('cascades')) cascades.add(id);

	for (const id of [...cascades].sort()) {
		await rebaseCascade(db, st, id, workFor(byCascade.get(id) ?? []), ctx, out);
	}

	// Preferences: the base row; the view is recomputed on every read.
	const tx = db.transaction(['preferences', 'meta'], 'readwrite');
	if (st.preferences) await tx.objectStore('preferences').put(st.preferences, 'row');
	if (ctx.advance !== false) {
		await writeMeta(tx as unknown as RwTx, 'sync', {
			cursor: st.seq,
			last_sync_at: ctx.now().toISOString(),
			upgrade_required: false
		});
	}
	await tx.done;
	return out;
}

async function rebaseCascade(db: UserDb, st: PullState, C: string, work: CascadeWork, ctx: RebaseCtx, out: RebaseOutcome) {
	const S = st.seq;
	const tx = db.transaction(REBASE_STORES, 'readwrite') as unknown as RwTx;
	tx.done.catch(() => undefined);
	const remaining = await tx.objectStore('outbox').index('cascade_id').getAll(C);
	const fast =
		!st.full &&
		!st.foreign.has(C) &&
		work.acked.every((a) => a.result.status === 'applied' && !CREATING.has(a.entry.op.type));
	let gone = false;
	const settled: Settled[] = work.acked.map((a) => ({
		entry: a.entry,
		status: a.result.status,
		reason: a.result.reason
	}));

	// Tombstones for this cascade.
	for (const t of st.tombstones.filter((t) => t.cascade_id === C)) {
		if (t.entity === 'cascade') {
			await deleteCascadeHere(tx, C);
			gone = true;
		} else {
			await tx.objectStore('quizzes').delete(t.entity_id);
			await deleteQuizRows(tx, t.entity_id);
		}
	}

	// A full pull removes what it did not carry and an earlier response wrote
	// (§ The sync cycle → Pull, 3): never a cascade with pending operations, and
	// never one a creation in flight wrote with a later sequence.
	const baseCascade = await tx.objectStore('cascades').get(C);
	if (st.full && !st.touched.has(C) && remaining.length === 0 && baseCascade && baseCascade.seq <= S) {
		await deleteCascadeHere(tx, C);
		gone = true;
	}

	// Step 2: the server's rows into the base.
	const promote = new Set<string>();
	if (!gone) {
		const staged = await tx.objectStore('staging_cascades').get(C);
		if (staged) await tx.objectStore('cascades').put(staged);
		for (const pq of await tx.objectStore('staging_quizzes').index('cascade_id').getAll(C)) {
			await settleQuiz(tx, st, C, pq, work, promote, out);
			out.changed.add(C);
		}
		for (const a of await tx.objectStore('staging_quiz_attempts').index('cascade_id').getAll(C)) {
			await tx.objectStore('quiz_attempts').put(a);
		}
		if (st.full && st.touched.has(C) && remaining.length === 0) {
			for (const q of await tx.objectStore('quizzes').index('cascade_id').getAll(C)) {
				if (q.seq < S) {
					await tx.objectStore('quizzes').delete(q.id);
					await deleteQuizRows(tx, q.id);
				} else if (q.status === 'active' && st.qrf.has(C)) {
					const qs = tx.objectStore('quiz_questions');
					for (const r of await qs.index('quiz_id').getAll(q.id)) {
						if (r.grade !== null && r.seq < S) await qs.delete([r.quiz_id, r.question_idx]);
					}
				}
			}
			const qa = tx.objectStore('quiz_attempts');
			for (const a of await qa.index('cascade_id').getAll(C)) {
				if (a.seq < S) await qa.delete([a.quiz_id, a.attempt]);
			}
		}
		if (staged || st.touched.has(C)) out.changed.add(C);
	}
	if (!(await tx.objectStore('cascades').getKey(C))) gone = true;

	// Mismatches: an applied finish whose outcome differs from what this device computed.
	for (const s of settled) {
		const local = s.entry.local;
		const res = work.acked.find((a) => a.entry === s.entry)?.result;
		if (s.status !== 'applied' || !local || !res || !CREATING.has(s.entry.op.type)) continue;
		if (s.entry.op.type === 'restore_quiz') continue;
		if (res.outcome !== local.outcome) s.mismatch = true;
		else if (
			work.created.has(s.entry.op.new_quiz_id as string) &&
			res.new_quiz_question_count === local.new_quiz_question_count &&
			res.new_quiz_questions_hash !== local.new_quiz_questions_hash
		) {
			// The same count but a different set: refused promotion, with the notice.
			// When only the count differs, nothing is said (§ Conflicts).
			s.mismatch = true;
		}
	}

	if (fast) {
		// The fast path: delete the overlay rows no remaining operation touches.
		const quizzes = new Set<string>();
		const questions = new Set<string>();
		const whole = new Set<string>();
		for (const e of remaining) {
			const op = e.op;
			if (op.quiz_id) quizzes.add(op.quiz_id as string);
			if (op.new_quiz_id) {
				quizzes.add(op.new_quiz_id as string);
				whole.add(op.new_quiz_id as string);
			}
			if (op.type === 'grade') questions.add(`${op.quiz_id}:${op.question_idx}`);
			if (CREATING.has(op.type)) whole.add(op.quiz_id as string);
		}
		await clearOverlay(tx, C, (store, row) => {
			const r = row as { id?: string; quiz_id?: string; question_idx?: number };
			if (store === 'cascades') return remaining.length > 0;
			if (store === 'overlay_quizzes') return quizzes.has(r.id!);
			if (store === 'overlay_quiz_attempts') return whole.has(r.quiz_id!);
			return whole.has(r.quiz_id!) || questions.has(`${r.quiz_id}:${r.question_idx}`);
		});
	} else {
		// Step 3: promote the rows this device built for a quiz an applied
		// operation created or reset, then delete the rest of the overlay.
		const oq = tx.objectStore('overlay_quiz_questions');
		for (const quizId of promote) {
			for (const r of await oq.index('quiz_id').getAll(quizId)) {
				await tx.objectStore('quiz_questions').put({ ...r, seq: S });
			}
		}
		await clearOverlay(tx, C);
		// Step 4: replay the outbox, in device_seq order, with the same rules.
		const device = await readMeta(tx, 'device');
		for (const e of remaining) {
			try {
				await applyOp(tx, e.op, {
					device_id: device.device_id,
					now: ctx.now().toISOString(),
					max_quiz_questions: ctx.max_quiz_questions
				});
			} catch (err) {
				if (!(err instanceof Rejected)) throw err;
				await tx.objectStore('outbox').delete(e.device_seq);
				settled.push({ entry: e, status: 'rejected', reason: err.reason });
			}
		}
	}

	// Staging rows of this cascade are done with.
	for (const store of ['staging_quizzes', 'staging_quiz_attempts', 'staging_quiz_questions'] as const) {
		const s = tx.objectStore(store);
		for (const k of await s.index('cascade_id').getAllKeys(C)) await s.delete(k as never);
	}
	await tx.objectStore('staging_cascades').delete(C);

	const quizIds = new Set((await tx.objectStore('quizzes').index('cascade_id').getAllKeys(C)) as string[]);
	const serverQuizzes = new Map<string, QuizRow>();
	for (const q of await tx.objectStore('quizzes').index('cascade_id').getAll(C)) serverQuizzes.set(q.id, q);
	await tx.done;

	out.notices.push(
		...noticesFor(C, settled, {
			cascadeGone: gone,
			quizExists: (id) => quizIds.has(id),
			passedRun: (e) => {
				const q = serverQuizzes.get(e.op.quiz_id as string);
				return !!q && q.attempt === (e.op.attempt as number) && q.cursor >= (e.op.segment_end as number);
			}
		})
	);
}

/**
 * Step 2 for one pulled quiz: promotion, a rebuild when its attempt or seed
 * differs from the base's (or the base lacks it), a materialisation when its
 * rows have just become complete, or the pending flag.
 */
async function settleQuiz(
	tx: RwTx,
	st: PullState,
	C: string,
	pq: QuizRow,
	work: CascadeWork,
	promote: Set<string>,
	out: RebaseOutcome
) {
	const S = st.seq;
	const base = tx.objectStore('quizzes');
	const qs = tx.objectStore('quiz_questions');
	const staged = await tx.objectStore('staging_quiz_questions').index('quiz_id').getAll(pq.id);
	const bq = await base.get(pq.id);
	const n = pq.question_count;
	const graded = pq.correct_count + pq.missed_count;

	// A quiz the pull reports cleared: its rows and positions stay as they are.
	if (pq.status === 'cleared') {
		await base.put(mergeQuiz(pq, false));
		return;
	}

	// Promotion: the device's rows for a quiz an applied operation created or
	// reset, when the pulled row's attempt and seed (and, for a created quiz,
	// its question hash) equal those of the rows the device built.
	const ovq = await tx.objectStore('overlay_quizzes').get(pq.id);
	if (ovq && !ovq.pending && ovq.attempt === pq.attempt && ovq.shuffle_seed === pq.shuffle_seed) {
		const created = work.created.has(pq.id);
		if ((created && ovq.questions_hash === pq.questions_hash) || (!created && work.reset.has(pq.id))) {
			await base.put(mergeQuiz(pq, false));
			promote.add(pq.id);
			return;
		}
	}

	const seedChanged = !!bq && (bq.attempt !== pq.attempt || bq.shuffle_seed !== pq.shuffle_seed);
	if (bq?.pending) {
		// A pending quiz stays pending whatever rows the base holds; pulled
		// grade rows are never used to rebuild it.
		await base.put(mergeQuiz(pq, true));
		return;
	}
	const inQrf = st.qrf.has(C);
	if (bq && !seedChanged && staged.length === 0) {
		// The usual case, answered from counts: an unchanged attempt whose rows
		// are all here with positions needs nothing, however large the quiz.
		const total = await qs.index('quiz_id').count(pq.id);
		const positioned = await qs.index('quiz_position').count(IDBKeyRange.bound([pq.id, -Infinity], [pq.id, Infinity]));
		if (total === n && positioned === n) {
			await base.put(mergeQuiz(pq, false));
			return;
		}
	}
	let rows = await qs.index('quiz_id').getAll(pq.id);
	let idx: number[] | null = null;
	if (pq.origin === 'source' && (inQrf || rows.length > 0)) idx = Array.from({ length: n }, (_, i) => i);
	else if (rows.length === n) idx = rows.map((r) => r.question_idx).sort((a, b) => a - b);

	if (seedChanged) {
		// A rebuild: rows reset, grades cleared, then the pulled grades on top.
		for (const r of rows) await qs.delete([r.quiz_id, r.question_idx]);
		rows = idx
			? idx.map((i) => ({ quiz_id: pq.id, question_idx: i, cascade_id: C, position: null, grade: null, graded_at: null, seq: S }))
			: [];
	} else if (idx && rows.length < n) {
		// A Source quiz new to this device: its rows are idx 0 … count − 1, made here.
		const have = new Map(rows.map((r) => [r.question_idx, r]));
		rows = idx.map(
			(i) => have.get(i) ?? { quiz_id: pq.id, question_idx: i, cascade_id: C, position: null, grade: null, graded_at: null, seq: S }
		);
	}
	const byIdx = new Map(rows.map((r) => [r.question_idx, r]));
	for (const g of staged) {
		const r = byIdx.get(g.question_idx);
		byIdx.set(g.question_idx, { ...(r ?? g), grade: g.grade, graded_at: g.graded_at, seq: S });
	}
	rows = [...byIdx.values()];
	if (!idx && rows.length === n && n > 0) idx = rows.map((r) => r.question_idx).sort((a, b) => a - b);

	let complete = false;
	if (idx) {
		// A rebuild or a materialisation runs only once the rows are complete,
		// then hashes them against the quiz row.
		if (questionsHash(idx).toString() === pq.questions_hash) {
			rows = withPositions(rows, pq.shuffle_seed).map((r) => ({ ...r, seq: S }));
			complete = true;
		} else {
			for (const r of rows) await qs.delete([r.quiz_id, r.question_idx]);
			rows = [];
			out.needsIndexList.add(pq.id);
		}
	}
	for (const r of rows) await qs.put(r);
	if (!complete && inQrf) out.needsIndexList.add(pq.id);
	const gradedHere = rows.filter((r) => r.grade !== null).length;
	await base.put(mergeQuiz(pq, graded > 0 && gradedHere < graded));
}
