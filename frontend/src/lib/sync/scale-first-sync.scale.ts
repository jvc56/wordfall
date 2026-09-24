// PLAN.md § Scale tests: "A new device logging into an account with ten such
// cascades completes its first sync in under 5 seconds with no question rows
// transferred, since none is opened here; after opening all ten, with each
// Source quiz finished once, creating a 150,000-question Level 2, then fully
// graded again on its new attempt without finishing, and that Level 2 half
// graded (375,000 graded rows per cascade, about 3.75 million in all, 75
// pages), the sync that follows finishes in under 120 seconds on the CI
// runner, measured with the staging split (rows of quizzes new to the device
// and grade rows for unchanged attempts written once); and each cascade
// materialises on open within budget."
import { describe, expect, it } from 'vitest';
import { FakeServer } from './testing/fake-server';
import { ids, QUESTIONS, report, ScaleDev, ServerPlayer, timed } from './testing/scale';

const CASCADES = 10;
const FIRST_SYNC_BUDGET_MS = 5_000;
const OPEN_SYNC_BUDGET_MS = 120_000;
/** One cascade's open: its two active quizzes' rows, keys and answers. */
const MATERIALISE_BUDGET_MS = 300_000;
const GRADED_PER_CASCADE = QUESTIONS + QUESTIONS / 4;

describe('a new device on an account with ten 300,000-question cascades', () => {
	it('first syncs with no question rows, then pulls 3.75 million graded rows in 75 pages, and materialises each', async () => {
		const server = new FakeServer();
		const all = Array.from({ length: CASCADES }, (_, i) => ({ ...ids(i + 1), level2: `b0000000-0000-4000-8000-${(i + 1).toString(16).padStart(12, '0')}` }));
		for (const { cascade, source } of all) server.create({ id: cascade, source_id: source, count: QUESTIONS });

		// The first sync: nothing opened, so no question rows travel.
		const d = await ScaleDev.make(server);
		const [, firstMs] = await timed(() => d.engine.sync());
		const rowsSent = d.responses.reduce((n, r) => n + (r.changes?.quiz_questions ?? []).reduce((m, g) => m + g.question_idx.length, 0), 0);
		report('first sync', firstMs, FIRST_SYNC_BUDGET_MS, `${d.requests.length} requests, ${rowsSent} question rows`);
		expect(rowsSent).toBe(0);
		expect(await d.count('cascades')).toBe(CASCADES);
		expect(firstMs).toBeLessThan(FIRST_SYNC_BUDGET_MS);

		// All ten opened here; meanwhile another device plays each.
		await d.open(...all.map((x) => x.cascade));
		const other = new ServerPlayer(server);
		for (const { cascade, source, level2 } of all) {
			// Finished once (half missed): a 150,000-question Level 2.
			other.grade(cascade, source, (p) => p % 2 === 1);
			const r = other.finish(cascade, source, 99n, level2);
			expect(r.new_quiz_question_count).toBe(QUESTIONS / 2);
			// The Source quiz fully graded again on its new attempt, not finished;
			// Level 2 half graded.
			other.grade(cascade, source, () => false);
			other.grade(cascade, level2, () => false, 0, QUESTIONS / 4);
		}

		// The sync that follows carries every graded row of their active quizzes.
		const before = d.requests.length;
		const [, openMs] = await timed(() => d.engine.sync());
		const pages = d.requests.length - before;
		const graded = await d.count('quiz_questions');
		report('sync after opening ten', openMs, OPEN_SYNC_BUDGET_MS, `${graded} graded rows in ${pages} pages`);
		expect(graded).toBe(CASCADES * GRADED_PER_CASCADE);
		// 75 pages of 50,000 graded rows, the plan's figure, plus the page the
		// changed cascade, quiz and attempt rows push over.
		expect(pages).toBeGreaterThanOrEqual(75);
		expect(pages).toBeLessThanOrEqual(76);
		expect(openMs).toBeLessThan(OPEN_SYNC_BUDGET_MS);

		// Each cascade materialises on open within budget.
		for (const { cascade } of all) {
			const [, ms] = await timed(() => d.dl.ensureCascade(cascade));
			report('materialise on open', ms, MATERIALISE_BUDGET_MS, cascade);
			expect(ms).toBeLessThanOrEqual(MATERIALISE_BUDGET_MS);
		}
	});
});
