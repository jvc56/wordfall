// PLAN.md § Unit tests → Frontend, the player's cases: the three actions,
// Previous and run boundaries, a card whose key never arrived, a card whose
// answer is gone (typed falls back to flashcard mode), typed mode, a missing
// distribution, the quota path on applyLocally, and a segmented attempt
// played through as the player emits it.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { beforeEach, describe, expect, it } from 'vitest';
import { storeCreated } from '$lib/local/created';
import { openUserDb, type UserDb } from '$lib/local/db';
import { initMeta } from '$lib/local/meta';
import { defaultPreferences } from '$lib/local/preferences';
import type { PreferencesRow } from '$lib/local/rows';
import { questionsHash } from '$lib/cascade/order';
import { Player } from './controller';
import { bind, inputOwns, actionFor, Debounce } from './bindings';
import { DEFAULT_BINDINGS } from '$lib/local/preferences';
import { finishBanner } from './banner';

const C = 'c1000000-0000-4000-8000-000000000000';
const Q = 'a1000000-0000-4000-8000-000000000000';
const T0 = '2026-01-01T00:00:00.000Z';
const WORDS = ['AB', 'BA'];

let db: UserDb;
let prefs: PreferencesRow;

beforeEach(async () => {
	globalThis.indexedDB = new IDBFactory();
	db = await openUserDb('u');
	await initMeta(db, 'u', 'u');
	prefs = defaultPreferences();
});

async function cascade(count: number, o: { segment_size?: number; keys?: boolean; cards?: boolean; dist?: boolean } = {}) {
	const idx = Array.from({ length: count }, (_, i) => i);
	const opts = { segment_size: o.segment_size ?? 0, progression: 'ladder', require_alphabetical: false };
	await storeCreated(db, {
		cascade: {
			id: C, name: 'x', quiz_type: 'anagram', lexicon: 'EN', letter_distribution: 'english', clear_threshold: 80, ...opts,
			options_changed_at: T0, options_seq: '1', options_device_id: 'd', question_count: count, depth: 1, peak_depth: 1,
			attempts_since_completion: 0, created_at: T0, last_activity_at: T0, completed_at: null, trashed_at: null, updated_seq: '1'
		},
		source_quiz: {
			id: Q, cascade_id: C, level: 1, origin: 'source', origin_quiz_id: null, origin_attempt: null, origin_segment_end: null,
			status: 'active', segment_chain: false, ...opts, options_changed_at: T0, options_seq: '1', options_device_id: 'd',
			attempt: 1, shuffle_seed: '12345', questions_hash: questionsHash(idx).toString(), question_count: count, correct_count: 0,
			missed_count: 0, cursor: 0, cursor_moved_at: null, cursor_device_id: null, run_start: 0, created_at: T0, created_seq: '1',
			last_activity_at: T0, cleared_at: null, updated_seq: '1'
		},
		sync_seq: '1'
	});
	for (const i of idx) {
		if (o.keys !== false) await db.put('questions', { cascade_id: C, idx: i, key: 'AB' });
		if (o.cards !== false) {
			await db.put('cards', { cascade_id: C, idx: i, answer: WORDS.map((word) => ({ word })), definitions: false, hooks: false });
		}
	}
	if (o.dist !== false) {
		const tiles = [{ letter: '?', blank_letter: '?', count: 2, value: 0, is_vowel: false }];
		for (const L of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') tiles.push({ letter: L, blank_letter: L.toLowerCase(), count: 2, value: 1, is_vowel: 'AEIOU'.includes(L) });
		await db.put('distributions', { name: 'english', tiles });
	}
}

function player() {
	return new Player(db, C, { prefs: async () => prefs, ensure: async () => undefined, seed: () => '777' });
}

const ops = async () => (await db.getAll('outbox')).map((e) => e.op.type);

describe('the player', () => {
	it('Show / Next reveals Correct, Toggle flips it, and Show / Next saves the grade and moves the cursor', async () => {
		await cascade(3);
		const p = player();
		await p.load();
		expect(p.v.card!.revealed).toBe(false);
		await p.showNext();
		expect([p.v.card!.revealed, p.v.card!.grade]).toEqual([true, 'correct']);
		await p.toggle();
		expect(p.v.card!.grade).toBe('missed');
		await p.showNext();
		expect(await ops()).toEqual(['grade', 'move_cursor']);
		expect(p.v.card!.pos).toBe(1);
		expect(p.v.card!.revealed).toBe(false);
	});

	it('a card toggled before the reveal starts Missed; Previous shows the earlier card with its saved grade and discards the unsaved reveal', async () => {
		await cascade(3);
		const p = player();
		await p.load();
		await p.toggle();
		await p.showNext();
		expect(p.v.card!.grade).toBe('missed');
		await p.showNext();
		await p.showNext(); // reveal card 2
		await p.previous();
		expect(p.v.card!.pos).toBe(0);
		expect([p.v.card!.revealed, p.v.card!.grade]).toEqual([true, 'missed']);
		// Previous does nothing on the first card, and says why when it cannot.
		await p.previous();
		expect(p.v.card!.pos).toBe(0);
	});

	it('the last card finishes the attempt with its banner', async () => {
		await cascade(2);
		const p = player();
		await p.load();
		await p.showNext();
		await p.showNext();
		await p.showNext();
		await p.toggle();
		await p.showNext();
		expect((await ops()).at(-1)).toBe('finish');
		expect(p.v.banner).toBe('Level 1: 50%. Reshuffled, and its 1 missed questions are now Level 2.');
		expect(p.v.quiz!.level).toBe(2);
	});

	it('a segmented attempt played as the player emits it: runs, drills and the finish, with no rejection', async () => {
		await cascade(10, { segment_size: 5 });
		const p = player();
		await p.load();
		const banners: string[] = [];
		for (let step = 0; step < 60 && p.v.status === 'ready'; step++) {
			const q = p.v.quiz!;
			await p.showNext();
			// Miss the first card of each Level 1 run.
			if (q.level === 1 && p.v.card!.pos % 5 === 0) await p.toggle();
			await p.showNext();
			if (p.v.banner) banners.push(p.v.banner);
			if (p.v.banner?.startsWith('Level 1:')) break;
		}
		expect(banners).toEqual([
			'Run 1 of 2 done, 1 missed. Down to Level 2 to drill them.',
			'Level 2 done. Back to Level 1, run 2 of 2.',
			'Level 1: 80%. Reshuffled, and its 2 missed questions are now Level 2.'
		]);
	});

	it('a card whose key never arrived needs a connection, emits nothing, and the run cannot finish', async () => {
		await cascade(3, { keys: false });
		await db.put('questions', { cascade_id: C, idx: 0, key: 'AB' });
		await db.put('questions', { cascade_id: C, idx: 2, key: 'AB' });
		const p = player();
		await p.load();
		// Walk through: the card without a key is moved past with no operation.
		for (let i = 0; i < 6; i++) {
			if (p.v.card!.key === null) {
				expect(p.v.card!.key).toBeNull();
				const before = (await ops()).length;
				await p.showNext();
				expect((await ops()).length).toBe(before);
			} else {
				await p.showNext();
				await p.showNext();
			}
		}
		expect(await ops()).not.toContain('finish');
		expect(p.v.message).toMatch(/^Some questions in this run still need a connection \(1\)\.$/);
	});

	it('a typed card whose answer is gone falls back to flashcard mode: Correct to start, Toggle decides, with the reveal saying so', async () => {
		await cascade(2, { cards: false });
		prefs = { ...prefs, anagram_answer_mode: 'typed' };
		const p = player();
		await p.load();
		expect(p.v.card!.typed).toBeNull();
		expect(p.v.card!.answer).toBeNull();
		await p.showNext();
		expect(p.v.card!.grade).toBe('correct');
		await p.toggle();
		await p.showNext();
		expect((await db.getAll('outbox'))[0].op.grade).toBe('missed');
	});

	it('typed mode: found, already entered, one word at a time, wrong; all found reveals', async () => {
		await cascade(2);
		prefs = { ...prefs, anagram_answer_mode: 'typed' };
		const p = player();
		await p.load();
		expect((await p.enter('ab'))!.kind).toBe('found');
		expect((await p.enter('AB'))!.kind).toBe('already');
		expect((await p.enter('ABAB'))!.kind).toBe('one_word');
		expect((await p.enter('AA'))!.kind).toBe('wrong');
		expect((await p.enter('BA'))!.kind).toBe('found');
		expect([p.v.card!.revealed, p.v.card!.grade]).toEqual([true, 'missed']);
	});

	it('typed mode with alphabetical order: an entry before the furthest is found but out of order', async () => {
		await cascade(2);
		const q = (await db.get('quizzes', Q))!;
		await db.put('quizzes', { ...q, require_alphabetical: true });
		prefs = { ...prefs, anagram_answer_mode: 'typed' };
		const p = player();
		await p.load();
		await p.enter('BA');
		expect((await p.enter('AB'))!.kind).toBe('out_of_order');
		expect(p.v.card!.grade).toBe('missed');
	});

	it('typed mode on a cascade whose distribution is missing needs a connection and falls back', async () => {
		await cascade(2, { dist: false });
		prefs = { ...prefs, anagram_answer_mode: 'typed' };
		const p = player();
		await p.load();
		expect(p.v.distributionMissing).toBe(true);
		expect(p.v.card!.typed).toBeNull();
	});

	it('a QuotaExceededError from applyLocally keeps the card with the storage message until space is freed', async () => {
		await cascade(3);
		const p = player();
		await p.load();
		await p.showNext();
		const real = db.transaction.bind(db);
		let full = true;
		(db as unknown as { transaction: typeof real }).transaction = ((...a: Parameters<typeof real>) => {
			if (full && a[1] === 'readwrite') throw new DOMException('full', 'QuotaExceededError');
			return real(...a);
		}) as typeof real;
		await p.showNext();
		expect(p.v.storage).toBe(true);
		expect(p.v.card!.pos).toBe(0);
		expect(await db.count('outbox')).toBe(0);
		await p.showNext(); // Show / Next advances nothing
		expect(p.v.card!.pos).toBe(0);
		full = false;
		await p.retryStorage();
		expect(p.v.storage).toBe(false);
		expect(await ops()).toEqual(['grade', 'move_cursor']);
		expect(p.v.card!.pos).toBe(1);
	});
});

describe('controls', () => {
	it('matches strokes with their exact modifiers and protects typing', () => {
		const key = (code: string, extra: Partial<KeyboardEvent> = {}) =>
			({ code, key: code === 'Space' ? ' ' : code.replace('Key', '').toLowerCase(), ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...extra }) as KeyboardEvent;
		const s = { kind: 'key' as const, code: 'Space', ctrl: false, shift: false, alt: false, meta: false };
		expect(actionFor(DEFAULT_BINDINGS, s)).toBe('show_next');
		expect(actionFor(DEFAULT_BINDINGS, { ...s, ctrl: true })).toBeNull();
		expect(inputOwns(key('KeyT', { shiftKey: true }))).toBe(true);
		expect(inputOwns(key('Enter', { key: 'Enter' }))).toBe(true);
		expect(inputOwns(key('KeyT', { ctrlKey: true }))).toBe(false);
		expect(inputOwns(key('Escape', { key: 'Escape' }))).toBe(false);
		expect(inputOwns(key('ArrowLeft', { key: 'ArrowLeft' }))).toBe(false);
	});

	it('a stroke belongs to one action, every action keeps one binding, and Escape cannot be bound', () => {
		const x = { kind: 'key' as const, code: 'KeyX', ctrl: false, shift: false, alt: false, meta: false };
		const moved = bind(DEFAULT_BINDINGS, 'previous', x);
		expect(moved.moved).toBe('toggle_grade');
		expect(moved.bindings.filter((b) => b.code === 'KeyX').map((b) => b.action)).toEqual(['previous']);
		const esc = bind(DEFAULT_BINDINGS, 'previous', { ...x, code: 'Escape' });
		expect(esc.refused).toBe(true);
	});

	it('the same action twice within 120 ms counts once', () => {
		const d = new Debounce(120);
		expect(d.pass('show_next', 1000)).toBe(true);
		expect(d.pass('show_next', 1100)).toBe(false);
		expect(d.pass('show_next', 1121)).toBe(true);
	});
});

describe('banners', () => {
	it('say what finishing did', () => {
		const base = { level: 2, correct: 87, count: 100, misses: 13, threshold: 80, progression: 'ladder' as const, segmentChain: false };
		expect(finishBanner({ ...base, outcome: 'cleared' })).toBe('Level 2 cleared with 87%. Its 13 missed questions are now Level 2.');
		expect(finishBanner({ ...base, correct: 64, misses: 36, outcome: 'descended' })).toBe(
			'Level 2: 64%, and 80% is needed to clear. Level 2 is reshuffled and waiting. Down to Level 3 with 36 missed questions.'
		);
		expect(finishBanner({ ...base, correct: 64, misses: 36, outcome: 'replaced' })).toBe('Level 2: 64%. Replaced with its 36 missed questions.');
		expect(finishBanner({ ...base, correct: 0, outcome: 'reshuffled' })).toBe('Level 2: 0%. Reshuffled. Try again.');
		expect(finishBanner({ ...base, correct: 100, misses: 0, outcome: 'cleared' })).toBe('Level 2 cleared with 100%. Back to Level 1.');
		expect(finishBanner({ ...base, level: 1, correct: 100, misses: 0, outcome: 'completed', completion: { levels: 4, attempts: 9 } })).toBe(
			'Level 1: 100%. Cascade complete after 4 levels and 9 attempts.'
		);
	});
});
