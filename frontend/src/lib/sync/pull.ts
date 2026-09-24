// Writing a pull's pages as they arrive (PLAN.md § The sync cycle → Applying
// a pull on the device). Question rows go straight to the base when the
// player could not be using an order they might disturb — every row of a quiz
// with no base row, and grade rows for an unchanged attempt of a quiz that is
// not pending — and nowhere at all for a pending quiz, whose rows the grades
// endpoint brings. Everything else waits in staging (tombstones and the
// preferences row in memory) for the rebase at the last page. Every row
// written carries `seq`, the sequence this pull took.
import type { UserDb } from '$lib/local/db';
import {
	attemptFromWire,
	cascadeFromWire,
	preferencesFromWire,
	quizFromWire,
	type PreferencesRow,
	type QuestionRow,
	type QuizRow
} from '$lib/local/rows';
import { STAGING } from '$lib/local/db';
import type { Changes, Tombstone } from './protocol';

export interface PullState {
	/** The sequence the first page's push took: every page's ceiling. */
	seq: number;
	/** A `cursor: null` pull. */
	full: boolean;
	/** `question_rows_for` as the first page sent it. */
	qrf: Set<string>;
	/** Cascades with any row in this pull, tombstones included. */
	touched: Set<string>;
	/**
	 * Cascades with a pulled row not stamped by this push: an `updated_seq`, a
	 * question group's `min_updated_seq` or a tombstone's `seq` other than
	 * `seq`, read from the rows themselves (the fast-path test).
	 */
	foreign: Set<string>;
	tombstones: (Tombstone & { cascade_id: string | null })[];
	preferences: PreferencesRow | null;
	/** Pulled quiz → its cascade, for the rows that name only the quiz. */
	quizCascade: Map<string, string>;
}

export function newPull(seq: number, full: boolean, qrf: string[]): PullState {
	return {
		seq,
		full,
		qrf: new Set(qrf),
		touched: new Set(),
		foreign: new Set(),
		tombstones: [],
		preferences: null,
		quizCascade: new Map()
	};
}

/** Staging is cleared at the start of every pull, so an interrupted one leaves nothing behind. */
export async function clearStaging(db: UserDb) {
	const tx = db.transaction([...STAGING], 'readwrite');
	await Promise.all(STAGING.map((s) => tx.objectStore(s).clear()));
	await tx.done;
}

const n = (v: unknown) => Number(v);

export async function writePage(db: UserDb, st: PullState, changes: Changes) {
	const tx = db.transaction(
		['quizzes', 'quiz_questions', 'staging_cascades', 'staging_quizzes', 'staging_quiz_questions', 'staging_quiz_attempts'],
		'readwrite'
	);
	tx.done.catch(() => undefined);
	const S = st.seq;
	const mark = (cascade: string | undefined | null, rowSeq: number) => {
		if (!cascade) return;
		st.touched.add(cascade);
		if (rowSeq !== S) st.foreign.add(cascade);
	};
	const cascadeOfQuiz = async (quizId: string): Promise<string | undefined> =>
		st.quizCascade.get(quizId) ?? (await tx.objectStore('quizzes').get(quizId))?.cascade_id;

	for (const w of changes.cascades) {
		const c = cascadeFromWire(w, S);
		mark(c.id, c.updated_seq);
		await tx.objectStore('staging_cascades').put(c);
	}
	for (const w of changes.quizzes) {
		const q = quizFromWire(w, S);
		st.quizCascade.set(q.id, q.cascade_id);
		mark(q.cascade_id, q.updated_seq);
		await tx.objectStore('staging_quizzes').put(q);
	}
	for (const w of changes.quiz_attempts) {
		const cascade = await cascadeOfQuiz(w.quiz_id as string);
		if (!cascade) continue;
		const a = attemptFromWire(w, cascade, S);
		mark(cascade, a.updated_seq);
		await tx.objectStore('staging_quiz_attempts').put(a);
	}
	for (const g of changes.quiz_questions) {
		const cascade = await cascadeOfQuiz(g.quiz_id);
		if (!cascade) continue;
		mark(cascade, n(g.min_updated_seq));
		const base = await tx.objectStore('quizzes').get(g.quiz_id);
		if (base?.pending) continue; // written nowhere: the grades endpoint brings a pending quiz its rows
		const pulled = await tx.objectStore('staging_quizzes').get(g.quiz_id);
		const unchanged = base && (!pulled || (pulled.attempt === base.attempt && pulled.shuffle_seed === base.shuffle_seed));
		const direct = !base || unchanged;
		const store = direct ? tx.objectStore('quiz_questions') : tx.objectStore('staging_quiz_questions');
		// Every row's read, then every write, issued together: IndexedDB pipelines
		// requests within a transaction, where awaiting each in turn pays a round
		// trip per row.
		const existing: (QuestionRow | undefined)[] = direct
			? await Promise.all(g.question_idx.map((idx) => store.get([g.quiz_id, idx])))
			: [];
		await Promise.all(
			g.question_idx.map((idx, i) =>
				store.put({
					quiz_id: g.quiz_id,
					question_idx: idx,
					cascade_id: cascade,
					position: existing[i]?.position ?? null,
					grade: g.grade[i],
					graded_at: g.graded_at[i],
					seq: S
				})
			)
		);
	}
	if (changes.preferences) st.preferences = preferencesFromWire(changes.preferences);
	for (const t of changes.tombstones) {
		const cascade = t.entity === 'cascade' ? t.entity_id : ((await cascadeOfQuiz(t.entity_id)) ?? null);
		mark(cascade, n(t.seq));
		st.tombstones.push({ ...t, cascade_id: cascade });
	}
	await tx.done;
}

/** A base quiz row with the pulled values and this device's own flags. */
export function mergeQuiz(pulled: QuizRow, pending: boolean): QuizRow {
	const q: QuizRow = { ...pulled };
	if (pending) q.pending = true;
	else delete q.pending;
	return q;
}
