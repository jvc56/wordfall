// PLAN.md § Scale tests, the device's half of the first bullet: "Create a
// 300,000-question cascade, download its cards, grade every question (half
// missed) through sync, finish, and assert the timings for creation,
// download, push (300,000 grades in 600 requests back to back …), pull and
// finish stay within budget, assert that the rebase work per acknowledged
// batch stays proportional to the batch (the drain's total IndexedDB writes
// within a small multiple of the operation count), and assert the IndexedDB
// row count of one copy plus a small overlay once the outbox has drained,
// recording the bytes of the rows, the question keys and the answers
// separately, with the keys near 3 MB." The server's half is backend/tests/scale.rs,
// which also times the pull; the device's pull at scale is scale-first-sync.
import { describe, expect, it } from 'vitest';
import { storeCreated } from '$lib/local/created';
import { OVERLAY } from '$lib/local/db';
import { FakeServer } from './testing/fake-server';
import { ids, QUESTIONS, report, ScaleDev, timed, writes } from './testing/scale';

// Budgets for disk-backed Chromium IndexedDB, about twice what the development
// machine measured (in brackets), which writes 5,000–15,000 indexed rows a second.
/** 300,000 Source rows with positions, written in one transaction [45 s]. */
const CREATE_BUDGET_MS = 120_000;
/** 3 keys pages and 30 answer pages into IndexedDB [74 s]. */
const DOWNLOAD_BUDGET_MS = 180_000;
/** 600 requests, each acknowledged batch rebased [425 s, before the in-memory server's own fix]. */
const PUSH_BUDGET_MS = 900_000;
/** The finish: 300,000 rows reset and a 150,000-question Level 2, then acknowledged [177 s]. */
const FINISH_BUDGET_MS = 400_000;
/** The drain's IndexedDB writes per operation it pushed. */
const WRITES_PER_OP = 6;
/** Overlay rows left once the outbox has drained. */
const OVERLAY_ROWS = 10;
const KEY_BYTES_MIN = 2.5e6;
const KEY_BYTES_MAX = 3.5e6;

describe('a 300,000-question cascade on the device', () => {
	it('creates, downloads, pushes 600 batches with proportional rebase work, pulls and finishes within budget', async () => {
		const server = new FakeServer();
		const { cascade, source } = ids(1);
		const d = await ScaleDev.make(server);

		// Creation: the server's reply stored as the device stores it (the
		// in-memory server's own work is not the device's, so it is outside the timing).
		const created = server.create({ id: cascade, source_id: source, count: QUESTIONS });
		const [, createMs] = await timed(async () => {
			await storeCreated(d.db, created);
			await d.open(cascade);
		});
		report('creation', createMs, CREATE_BUDGET_MS, `${QUESTIONS} questions`);
		expect(createMs).toBeLessThanOrEqual(CREATE_BUDGET_MS);

		// Download: positions (implied for a Source quiz), keys, then answers.
		const [, downloadMs] = await timed(() => d.sync());
		const keyCalls = d.calls.filter((c) => c.kind === 'keys');
		const cardCalls = d.calls.filter((c) => c.kind === 'cards');
		report('download', downloadMs, DOWNLOAD_BUDGET_MS, `${keyCalls.length} keys pages, ${cardCalls.length} answer pages`);
		expect([keyCalls.length, cardCalls.length]).toEqual([3, 30]);
		expect(await d.countFor('quiz_questions', cascade)).toBe(QUESTIONS);
		expect(await d.countFor('questions', cascade)).toBe(QUESTIONS);
		expect(await d.countFor('cards', cascade)).toBe(QUESTIONS);
		expect(downloadMs).toBeLessThanOrEqual(DOWNLOAD_BUDGET_MS);

		// Every question graded here, half missed, then the outbox drained. The
		// grades are queued in one transaction (what 300,000 applyLocally calls
		// leave; testing/scale.test.ts): the budget is the drain, not the tapping.
		const [, gradeMs] = await timed(() => d.queueGrades(source, (p) => p % 2 === 1));
		report('queueing 300,000 grades (setup)', gradeMs, null, `${await d.count('outbox')} operations queued`);
		expect(await d.count('outbox')).toBe(QUESTIONS);
		const before = writes.n;
		const serverBefore = d.serverMs;
		const [requests, pushMs] = await timed(() => d.drain());
		const drainWrites = writes.n - before;
		const serverMs = d.serverMs - serverBefore;
		report('push', pushMs, PUSH_BUDGET_MS, `${requests} requests, ${drainWrites} IndexedDB writes for ${QUESTIONS} operations; ${(serverMs / 1000).toFixed(1)} s of it in the in-memory server`);
		expect(requests).toBe(QUESTIONS / 500);
		expect(drainWrites).toBeLessThanOrEqual(WRITES_PER_OP * QUESTIONS);
		expect(pushMs).toBeLessThanOrEqual(PUSH_BUDGET_MS);

		// One copy plus a small overlay.
		expect(await d.count('quiz_questions')).toBe(QUESTIONS);
		let overlay = 0;
		for (const s of OVERLAY) overlay += await d.count(s);
		expect(overlay).toBeLessThanOrEqual(OVERLAY_ROWS);
		const rowBytes = (await d.bytes('quiz_questions')) + (await d.bytes('quizzes')) + (await d.bytes('cascades'));
		const keyBytes = await d.bytes('questions', (v) => v.key);
		const answerBytes = await d.bytes('cards', (v) => v.answer);
		report('stored', 0, null, `rows ${(rowBytes / 1e6).toFixed(1)} MB, keys ${(keyBytes / 1e6).toFixed(2)} MB, answers ${(answerBytes / 1e6).toFixed(1)} MB, overlay ${overlay} rows`);
		expect(keyBytes).toBeGreaterThanOrEqual(KEY_BYTES_MIN);
		expect(keyBytes).toBeLessThanOrEqual(KEY_BYTES_MAX);

		// Finish: the Source quiz descends; its 150,000 misses are Level 2.
		const q = (await d.quiz(source))!;
		const level2 = 'b0000000-0000-4000-8000-000000000001';
		const [, finishMs] = await timed(async () => {
			await d.apply({ type: 'finish', quiz_id: source, attempt: q.attempt, attempt_seed: q.shuffle_seed, shuffle_seed: '4242', new_quiz_id: level2 });
			await d.drain();
		});
		const l2 = await d.db.get('quizzes', level2);
		report('finish', finishMs, FINISH_BUDGET_MS, `Level 2 of ${l2?.question_count} questions`);
		expect(l2?.question_count).toBe(QUESTIONS / 2);
		expect(finishMs).toBeLessThanOrEqual(FINISH_BUDGET_MS);
	});
});
