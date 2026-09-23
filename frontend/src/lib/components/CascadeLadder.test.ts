// @vitest-environment jsdom
// PLAN.md § Unit tests → Frontend: "renders CascadeLadder and the Cascades
// page's compact ladder for a cascade of 30 levels … both collapse the middle
// levels behind a count with Level 1 and the deepest level still shown, Show
// more expands them, and a 12-level cascade collapses nothing."
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { questionsHash } from '$lib/cascade/order';
import { applyLocally, APPLY_STORES } from '$lib/local/apply';
import { storeCreated } from '$lib/local/created';
import { openUserDb, type UserDb } from '$lib/local/db';
import { initMeta } from '$lib/local/meta';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { levelRows } from '$lib/player/ladder';
import CascadeLadder from './CascadeLadder.svelte';

const C = 'c1000000-0000-4000-8000-000000000000';
const Q = 'a1000000-0000-4000-8000-000000000000';
const T0 = '2026-01-01T00:00:00.000Z';

/** A cascade of `levels` levels, built by finishing each level with all but one question missed. */
async function deepCascade(levels: number) {
	globalThis.indexedDB = new IDBFactory();
	const db: UserDb = await openUserDb('u');
	await initMeta(db, 'u', 'u');
	const idx = Array.from({ length: levels }, (_, i) => i);
	const opts = { segment_size: 0, progression: 'ladder', require_alphabetical: false };
	await storeCreated(db, {
		cascade: {
			id: C, name: 'x', quiz_type: 'anagram', lexicon: 'EN', letter_distribution: 'english', clear_threshold: 100, ...opts,
			options_changed_at: T0, options_seq: '1', options_device_id: 'd', question_count: levels, depth: 1, peak_depth: 1,
			attempts_since_completion: 0, created_at: T0, last_activity_at: T0, completed_at: null, trashed_at: null, updated_seq: '1'
		},
		source_quiz: {
			id: Q, cascade_id: C, level: 1, origin: 'source', origin_quiz_id: null, origin_attempt: null, origin_segment_end: null,
			status: 'active', segment_chain: false, ...opts, options_changed_at: T0, options_seq: '1', options_device_id: 'd',
			attempt: 1, shuffle_seed: '5', questions_hash: questionsHash(idx).toString(), question_count: levels, correct_count: 0,
			missed_count: 0, cursor: 0, cursor_moved_at: null, cursor_device_id: null, run_start: 0, created_at: T0, created_seq: '1',
			last_activity_at: T0, cleared_at: null, updated_seq: '1'
		},
		sync_seq: '1'
	});
	let quizId = Q;
	for (let l = 1; l < levels; l++) {
		const tx = db.transaction(APPLY_STORES) as unknown as RwTx;
		const q = (await view.quiz(tx, quizId))!;
		const rows = await view.questionsOf(tx, quizId);
		for (const [i, r] of rows.entries()) {
			await applyLocally(db, {
				type: 'grade', quiz_id: quizId, attempt: q.attempt, attempt_seed: q.shuffle_seed, question_idx: r.question_idx,
				grade: i === 0 ? 'correct' : 'missed'
			});
		}
		const next = crypto.randomUUID();
		await applyLocally(db, { type: 'finish', quiz_id: quizId, attempt: q.attempt, attempt_seed: q.shuffle_seed, shuffle_seed: String(l), new_quiz_id: next });
		quizId = next;
	}
	const tx = db.transaction(APPLY_STORES) as unknown as RwTx;
	const c = (await view.cascade(tx, C))!;
	return levelRows(await view.quizzesOf(tx, C), c.depth, []);
}

const shown = (container: HTMLElement) => [...container.querySelectorAll('[data-level]')].map((e) => Number(e.getAttribute('data-level')));

describe('CascadeLadder', () => {
	it('collapses a 30-level cascade and expands it on Show more; the compact form likewise', async () => {
		const rows = await deepCascade(30);
		expect(rows).toHaveLength(30);
		const panel = render(CascadeLadder, { rows });
		expect(shown(panel.container)).toEqual([1, 28, 29, 30]);
		await fireEvent.click(panel.getByText(/Show more \(26 levels\)/));
		expect(shown(panel.container)).toHaveLength(30);
		panel.unmount();
		const compact = render(CascadeLadder, { rows, compact: true });
		expect(shown(compact.container)).toEqual([1, 30]);
		await fireEvent.click(compact.getByText('+28 more'));
		expect(shown(compact.container)).toHaveLength(30);
	}, 60_000);

	it('collapses nothing at twelve levels', async () => {
		const rows = await deepCascade(12);
		expect(shown(render(CascadeLadder, { rows }).container)).toHaveLength(12);
		expect(shown(render(CascadeLadder, { rows, compact: true }).container)).toHaveLength(12);
	});
});
