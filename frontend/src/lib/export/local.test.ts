// PLAN.md § Unit tests → Frontend: a questions-only export of a cascade whose
// answers were evicted still written from local data; a card whose key never
// arrived sending the export to the server; a Trash export of a cleared quiz
// holding only part of its rows, or a set failing the hash check, fetching them
// before it writes anything; definitions the cards lack, and a Definition
// export's hooks, coming from the server.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { beforeEach, describe, expect, it } from 'vitest';
import { questionsHash } from '$lib/cascade/order';
import { storeCreated } from '$lib/local/created';
import { openUserDb, type UserDb } from '$lib/local/db';
import { initMeta } from '$lib/local/meta';
import type { CascadeRow } from '$lib/local/rows';
import { formatExport } from './format';
import { localInput, serverChoices, type DeviceChoices } from './local';

const C = 'c1000000-0000-4000-8000-000000000000';
const Q = 'a1000000-0000-4000-8000-000000000000';
const T0 = '2026-01-01T00:00:00.000Z';
let db: UserDb;

async function setup(quizType = 'anagram', count = 3) {
	globalThis.indexedDB = new IDBFactory();
	db = await openUserDb('u');
	await initMeta(db, 'u', 'u');
	const idx = Array.from({ length: count }, (_, i) => i);
	const opts = { segment_size: 0, progression: 'ladder', require_alphabetical: false };
	await storeCreated(db, {
		cascade: {
			id: C, name: 'Sevens', quiz_type: quizType, lexicon: 'EN', letter_distribution: 'english', clear_threshold: 80, ...opts,
			options_changed_at: T0, options_seq: '1', options_device_id: 'd', question_count: count, depth: 1, peak_depth: 1,
			attempts_since_completion: 0, created_at: T0, last_activity_at: T0, completed_at: null, trashed_at: null, updated_seq: '1'
		},
		source_quiz: {
			id: Q, cascade_id: C, level: 1, origin: 'source', origin_quiz_id: null, origin_attempt: null, origin_segment_end: null,
			status: 'active', segment_chain: false, ...opts, options_changed_at: T0, options_seq: '1', options_device_id: 'd',
			attempt: 1, shuffle_seed: '9', questions_hash: questionsHash(idx).toString(), question_count: count, correct_count: 0,
			missed_count: 0, cursor: 0, cursor_moved_at: null, cursor_device_id: null, run_start: 0, created_at: T0, created_seq: '1',
			last_activity_at: T0, cleared_at: null, updated_seq: '1'
		},
		sync_seq: '1'
	});
	for (const i of idx) {
		await db.put('questions', { cascade_id: C, idx: i, key: `K${i}` });
		const answer = quizType === 'anagram' ? [{ word: `W${i}` }] : quizType === 'definition' ? `def ${i}` : i + 0.5;
		await db.put('cards', { cascade_id: C, idx: i, answer, definitions: false, hooks: false });
	}
	return (await db.get('cascades', C)) as CascadeRow;
}

const base: DeviceChoices = { scope: 'cascade', which: 'all', format: 'txt', lines: 'questions', columns: ['question', 'answer', 'grade'], order: 'study', decimals: 1 };

describe('exports built on the device', () => {
	it('write the questions from the questions store even when the answers were evicted', async () => {
		const c = await setup();
		await db.clear('cards');
		const r = await localInput(db, c, base);
		expect('input' in r && [...formatExport(r.input)].join('')).toBe('K0\nK1\nK2\n');
		expect(await localInput(db, c, { ...base, lines: 'answers' })).toEqual({ missing: 'cards' });
	});

	it('go to the server when a key never arrived', async () => {
		const c = await setup();
		await db.delete('questions', [C, 1]);
		expect(await localInput(db, c, base)).toEqual({ missing: 'keys' });
	});

	it('go to the server for definitions the cards lack, and for a Definition export’s hooks', async () => {
		let c = await setup();
		expect(await localInput(db, c, { ...base, format: 'csv', columns: ['question', 'definition'] })).toEqual({ missing: 'definitions' });
		c = await setup('definition');
		expect(await localInput(db, c, { ...base, format: 'csv', columns: ['question', 'hooks'] })).toEqual({ missing: 'hooks' });
		expect(serverChoices('definition', { ...base, format: 'csv', columns: ['question', 'answer'] })).toMatchObject({ definitions: true, columns: 'question,answer' });
	});

	it('need a quiz’s whole row set, with positions and a matching hash, before writing it', async () => {
		const c = await setup();
		const quiz: DeviceChoices = { ...base, scope: 'quiz', quiz_id: Q };
		expect('input' in (await localInput(db, c, quiz))).toBe(true);
		await db.delete('quiz_questions', [Q, 2]);
		expect(await localInput(db, c, quiz)).toEqual({ missing: 'rows' });
		await db.put('quiz_questions', { quiz_id: Q, question_idx: 7, cascade_id: C, position: 2, grade: null, graded_at: null, seq: 1 });
		expect(await localInput(db, c, quiz)).toEqual({ missing: 'rows' });
	});

	it('write a leave value at the chosen decimals with no plus sign', async () => {
		const c = await setup('leave_value');
		const r = await localInput(db, c, { ...base, lines: 'answers', decimals: 2 });
		expect('input' in r && [...formatExport(r.input)].join('')).toBe('0.50\n1.50\n2.50\n');
	});
});
