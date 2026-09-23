// A cascade this device created (PLAN.md § Downloads: "A cascade starts
// downloading the moment it is created"). The creation response's rows go
// into the base stamped with its `sync_seq`, so a full pull in flight never
// deletes them, with the Source quiz's rows made here (idx 0 … count − 1,
// positions from its seed), and the creation counts as an open.
import { shuffle } from '$lib/cascade/order';
import type { UserDb } from './db';
import { recordOpen } from './meta';
import { cascadeFromWire, quizFromWire } from './rows';
import type { RwTx } from './view';

export interface Created {
	cascade: Record<string, unknown>;
	source_quiz: Record<string, unknown>;
	sync_seq: string;
}

export async function storeCreated(db: UserDb, created: Created, now = new Date()) {
	const seq = Number(created.sync_seq);
	const c = cascadeFromWire(created.cascade, seq);
	const q = quizFromWire(created.source_quiz, seq);
	const tx = db.transaction(['cascades', 'quizzes', 'quiz_questions', 'meta'], 'readwrite');
	await tx.objectStore('cascades').put(c);
	await tx.objectStore('quizzes').put(q);
	const idx = Array.from({ length: q.question_count }, (_, i) => i);
	const store = tx.objectStore('quiz_questions');
	const order = shuffle(idx, BigInt(q.shuffle_seed));
	await Promise.all(
		order.map((i, p) =>
			store.put({ quiz_id: q.id, question_idx: i, cascade_id: c.id, position: p, grade: null, graded_at: null, seq })
		)
	);
	await recordOpen(tx as unknown as RwTx, c.id, now.toISOString());
	await tx.done;
}
