// PLAN.md § Scale tests: "A new device logging into an account with ten such
// cascades completes its first sync in under 5 seconds with no question rows
// transferred, since none is opened here; after opening all ten, with each
// Source quiz finished once, creating a 150,000-question Level 2, then fully
// graded again on its new attempt without finishing, and that Level 2 half
// graded (375,000 graded rows per cascade, about 3.75 million in all, 75
// pages), the sync that follows finishes in under 120 seconds on the CI
// runner, measured with the staging split …; and each cascade materialises on
// open within budget."
//
// The first sync runs at the plan's size. The sync after opening runs at a
// tenth of its rows (PQ-019): one of the ten cascades played as the plan says,
// 375,000 graded rows in 8 pages, against a tenth of the budget — 3.75 million
// rows is about 3.75 million IndexedDB writes, eight minutes and more at the
// ~7,500 indexed rows a second measured here, so the full case could not both
// run in minutes and say anything its tenth does not.
import { describe, expect, it } from 'vitest';
import { FakeServer } from './testing/fake-server';
import { ids, QUESTIONS, report, ScaleDev, ServerPlayer, timed } from './testing/scale';

const CASCADES = 10;
const FIRST_SYNC_BUDGET_MS = 5_000;
/** A tenth of the plan's 120 s, for a tenth of its rows (PQ-019). */
const OPEN_SYNC_BUDGET_MS = 12_000;
/** The played cascade's open: its two active quizzes' index lists, positions, keys and answers. */
const MATERIALISE_BUDGET_MS = 300_000;
const GRADED = QUESTIONS + QUESTIONS / 4;
const PAGE_ROWS = 50_000;

describe('a new device on an account with ten 300,000-question cascades', () => {
	it('first syncs with no question rows, then pulls 375,000 graded rows through the staging split, and materialises', async () => {
		const server = new FakeServer();
		const all = Array.from({ length: CASCADES }, (_, i) => ids(i + 1));
		for (const { cascade, source } of all) server.create({ id: cascade, source_id: source, count: QUESTIONS });

		// The first sync: nothing opened, so no question rows travel.
		const d = await ScaleDev.make(server);
		const [, firstMs] = await timed(() => d.engine.sync());
		const rowsSent = d.responses.reduce((n, r) => n + (r.changes?.quiz_questions ?? []).reduce((m, g) => m + g.question_idx.length, 0), 0);
		report('first sync', firstMs, FIRST_SYNC_BUDGET_MS, `${CASCADES} cascades, ${d.requests.length} requests, ${rowsSent} question rows`);
		expect(rowsSent).toBe(0);
		expect(await d.count('cascades')).toBe(CASCADES);
		expect(firstMs).toBeLessThan(FIRST_SYNC_BUDGET_MS);

		// All ten opened here; meanwhile another device plays the first: finished
		// once (half missed, a 150,000-question Level 2), fully graded again on its
		// new attempt without finishing, and Level 2 half graded.
		await d.open(...all.map((x) => x.cascade));
		const [played] = all;
		const level2 = 'b0000000-0000-4000-8000-000000000001';
		const other = new ServerPlayer(server);
		other.grade(played.cascade, played.source, (p) => p % 2 === 1);
		expect(other.finish(played.cascade, played.source, 99n, level2).new_quiz_question_count).toBe(QUESTIONS / 2);
		other.grade(played.cascade, played.source, () => false);
		other.grade(played.cascade, level2, () => false, 0, QUESTIONS / 4);

		// The sync that follows: Level 2 is new to the device and the Source
		// quiz's attempt changed, so its rows are written once, not staged twice.
		const before = d.requests.length;
		const [, openMs] = await timed(() => d.engine.sync());
		const pages = d.requests.length - before;
		const graded = await d.count('quiz_questions');
		report('sync after opening', openMs, OPEN_SYNC_BUDGET_MS, `${graded} graded rows in ${pages} pages`);
		expect(graded).toBe(GRADED);
		expect(pages).toBe(Math.ceil(GRADED / PAGE_ROWS));
		expect(openMs).toBeLessThan(OPEN_SYNC_BUDGET_MS);

		// It materialises on open within budget.
		const [, ms] = await timed(() => d.dl.ensureCascade(played.cascade));
		report('materialise on open', ms, MATERIALISE_BUDGET_MS, `${QUESTIONS} + ${QUESTIONS / 2} rows with positions, keys and answers`);
		expect(await d.countFor('quiz_questions', played.cascade)).toBe(QUESTIONS + QUESTIONS / 2);
		expect(ms).toBeLessThanOrEqual(MATERIALISE_BUDGET_MS);
	});
});
