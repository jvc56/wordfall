// The one read path (PLAN.md § Frontend → lib/local): a row is the overlay's
// version when there is one, else the base's, so the rest of the app never
// knows there are two stores. An overlay row marked `deleted` hides the base's.
import type { IDBPTransaction } from 'idb';
import type { StoreName, UserSchema } from './db';
import type { AttemptRow, CascadeRow, QuestionRow, QuizRow } from './rows';

export type AnyTx = IDBPTransaction<UserSchema, StoreName[], 'readonly' | 'readwrite'>;
export type RwTx = IDBPTransaction<UserSchema, StoreName[], 'readwrite'>;

/** Every store a read of the view needs. */
export const VIEW_STORES: StoreName[] = [
	'cascades',
	'quizzes',
	'quiz_questions',
	'quiz_attempts',
	'overlay_cascades',
	'overlay_quizzes',
	'overlay_quiz_questions',
	'overlay_quiz_attempts'
];

export async function cascade(tx: AnyTx, id: string): Promise<CascadeRow | undefined> {
	const o = await tx.objectStore('overlay_cascades').get(id);
	if (o) return o.deleted ? undefined : o;
	return tx.objectStore('cascades').get(id);
}

export async function quiz(tx: AnyTx, id: string): Promise<QuizRow | undefined> {
	const o = await tx.objectStore('overlay_quizzes').get(id);
	if (o) return o.deleted ? undefined : o;
	return tx.objectStore('quizzes').get(id);
}

/** Whether an id is taken by any quiz this device knows, a purged one included. */
export async function quizIdTaken(tx: AnyTx, id: string): Promise<boolean> {
	return (
		(await tx.objectStore('overlay_quizzes').getKey(id)) !== undefined ||
		(await tx.objectStore('quizzes').getKey(id)) !== undefined
	);
}

export async function quizzesOf(tx: AnyTx, cascadeId: string): Promise<QuizRow[]> {
	const out = new Map<string, QuizRow>();
	for (const q of await tx.objectStore('quizzes').index('cascade_id').getAll(cascadeId)) out.set(q.id, q);
	for (const q of await tx.objectStore('overlay_quizzes').index('cascade_id').getAll(cascadeId)) {
		if (q.deleted) out.delete(q.id);
		else out.set(q.id, q);
	}
	return [...out.values()];
}

export async function question(tx: AnyTx, quizId: string, idx: number): Promise<QuestionRow | undefined> {
	return (
		(await tx.objectStore('overlay_quiz_questions').get([quizId, idx])) ??
		(await tx.objectStore('quiz_questions').get([quizId, idx]))
	);
}

/** A quiz's question rows, ascending by index. */
export async function questionsOf(tx: AnyTx, quizId: string): Promise<QuestionRow[]> {
	const out = new Map<number, QuestionRow>();
	for (const r of await tx.objectStore('quiz_questions').index('quiz_id').getAll(quizId)) out.set(r.question_idx, r);
	for (const r of await tx.objectStore('overlay_quiz_questions').index('quiz_id').getAll(quizId)) out.set(r.question_idx, r);
	return [...out.values()].sort((a, b) => a.question_idx - b.question_idx);
}

export async function attemptsOf(tx: AnyTx, quizId: string): Promise<AttemptRow[]> {
	const out = new Map<number, AttemptRow>();
	for (const r of await tx.objectStore('quiz_attempts').index('quiz_id').getAll(quizId)) out.set(r.attempt, r);
	for (const r of await tx.objectStore('overlay_quiz_attempts').index('quiz_id').getAll(quizId)) out.set(r.attempt, r);
	return [...out.values()].sort((a, b) => a.attempt - b.attempt);
}
