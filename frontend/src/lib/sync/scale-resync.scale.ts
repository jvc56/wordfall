// PLAN.md § Scale tests: "Resync a device holding five 300,000-question
// cascades, one of them with cleared levels whose rows a Trash export
// fetched, and assert the full pull's base comparison and deletions finish
// within budget and keep those cleared rows. A sixth cascade has run a
// segmented attempt with a segment size of 5 over 20,000 questions, leaving
// 4,000 chain quizzes in the Trash, so its pull pages on quiz rows alone:
// assert the page count, that no page exceeds 50,000 rows, and that the
// resync stays within the same budget."
import { describe, expect, it } from 'vitest';
import type { SyncResponse } from './protocol';
import { FakeServer } from './testing/fake-server';
import { ids, QUESTIONS, report, ScaleDev, ServerPlayer, timed } from './testing/scale';

/** About three times the 31 s measured on the development machine. */
const RESYNC_BUDGET_MS = 90_000;
const PAGE_ROWS = 50_000;
const SEGMENTED = 20_000;
const SEGMENT = 5;
const CHAINS = SEGMENTED / SEGMENT;

const uuid = (prefix: string, n: number) => `${prefix}0000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;

function rowsIn(p: SyncResponse): number {
	const c = p.changes;
	if (!c) return 0;
	return (
		(c.cascades?.length ?? 0) +
		(c.quizzes?.length ?? 0) +
		(c.quiz_attempts?.length ?? 0) +
		(c.quiz_questions ?? []).reduce((n, g) => n + g.question_idx.length, 0) +
		(c.tombstones?.length ?? 0) +
		(c.preferences ? 1 : 0)
	);
}

describe('a resync of a device holding five 300,000-question cascades and 4,000 chain quizzes', () => {
	it('compares and deletes within budget, keeps the exported cleared rows, and pages under 50,000 rows', async () => {
		const server = new FakeServer();
		const big = Array.from({ length: 5 }, (_, i) => ids(i + 1));
		const seg = ids(6);
		for (const { cascade, source } of big) server.create({ id: cascade, source_id: source, count: QUESTIONS });
		server.create({ id: seg.cascade, source_id: seg.source, count: SEGMENTED, segment_size: SEGMENT });
		const other = new ServerPlayer(server);

		// The device holds all six: rows and positions (their keys and answers are beside the point).
		const d = await ScaleDev.make(server);
		d.refuse = (c) => c.kind === 'keys' || c.kind === 'cards';
		await d.open(...big.map((x) => x.cascade), seg.cascade);
		await d.engine.sync();
		await d.dl.run().catch(() => undefined);

		// The first cascade clears a 150,000-question Level 2 …
		const [first] = big;
		const level2 = uuid('b', 1);
		other.grade(first.cascade, first.source, (p) => p % 2 === 1);
		expect(other.finish(first.cascade, first.source, 11n, level2).new_quiz_question_count).toBe(QUESTIONS / 2);
		other.grade(first.cascade, level2, () => false);
		other.finish(first.cascade, level2, 12n, uuid('b', 2));
		await d.engine.sync();
		expect((await d.db.get('quizzes', level2))?.status).toBe('cleared');
		// … whose rows a Trash export fetches.
		const cascadeRow = (await d.db.get('cascades', first.cascade))!;
		expect(await d.dl.materialiseQuiz(cascadeRow, (await d.db.get('quizzes', level2))!, true)).toBe(true);
		const cleared = await d.db.countFromIndex('quiz_questions', 'quiz_id', level2);
		expect(cleared).toBe(QUESTIONS / 2);

		// The sixth runs its 4,000 segments, one miss each, every chain quiz cleared.
		// A quiz's last run ends with `finish`, never `finish_segment`, so the plan's
		// "4,000 chain quizzes" from 20,000 questions in fives are 3,999 (PQ-011):
		// the last run is left unfinished.
		for (let r = 0; r < CHAINS - 1; r++) {
			const chain = uuid('d', r);
			other.grade(seg.cascade, seg.source, (p) => p === r * SEGMENT, r * SEGMENT, SEGMENT);
			expect(other.finishSegment(seg.cascade, seg.source, (r + 1) * SEGMENT, BigInt(r + 1), chain).outcome).toBe('drilled');
			other.grade(seg.cascade, chain, () => false);
			other.finish(seg.cascade, chain, BigInt(r + 1), uuid('e', r));
		}
		await d.engine.sync();
		const chains = (await d.db.getAllFromIndex('quizzes', 'cascade_id', seg.cascade)).filter((q) => q.status === 'cleared');
		expect(chains.length).toBe(CHAINS - 1);

		// A purge the device never hears of, its tombstone pruned: the next sync is a resync.
		other.raw('purge_quiz', { quiz_id: uuid('d', 0) });
		server.pruneTombstones();
		const before = d.responses.length;
		const [, ms] = await timed(() => d.engine.sync());
		const pages = d.responses.slice(before).filter((r) => r.changes);
		const sizes = pages.map(rowsIn);
		const total = sizes.reduce((a, b) => a + b, 0);
		report('resync', ms, RESYNC_BUDGET_MS, `${pages.length} pages of ${sizes.join(', ')} rows`);
		expect(pages.length).toBe(Math.ceil(total / PAGE_ROWS));
		for (const n of sizes) expect(n).toBeLessThanOrEqual(PAGE_ROWS);
		// The deletion found by comparison, and the exported cleared rows kept.
		expect(await d.db.get('quizzes', uuid('d', 0))).toBeUndefined();
		expect(await d.db.countFromIndex('quiz_questions', 'quiz_id', level2)).toBe(cleared);
		expect(ms).toBeLessThanOrEqual(RESYNC_BUDGET_MS);
	});
});
