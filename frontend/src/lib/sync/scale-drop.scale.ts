// PLAN.md § Scale tests: "Let a 300,000-question cascade leave the download
// window with the automatic Keep offline turned off, and assert the drop pass
// finishes within budget and the base holds only its cascade, quiz and
// attempt rows. Run the same pass on forty such cascades all inside the
// window, every one of them over AUTO_KEEP_ROWS and so automatically kept,
// and assert it brings the base's rows and keys under ROW_STORAGE_BUDGET
// within budget, oldest last-open first … Mark one of the forty Keep offline
// by hand and assert it is the one cascade still whole at the end. Then run
// one more pull and assert that nothing is refetched … Run the same case on
// a cascade that also holds five cleared 150,000-question levels inside the
// window, asserting the row count before the pass counts them and the pass
// removes them too." See PQ-017 for what "still whole" and "only the kept
// one" are taken to mean.
import { describe, expect, it } from 'vitest';
import { getMeta } from '$lib/local/meta';
import { ROW_STORAGE_BUDGET, AUTO_KEEP_OFFLINE_ROWS } from './config';
import { ROW_BYTES } from './downloads';
import { setKeepOffline } from './keep';
import { policy } from './policy';
import { FakeServer } from './testing/fake-server';
import { DAY, ids, QUESTIONS, report, ScaleDev, ServerPlayer, timed } from './testing/scale';

const ONE_DROP_BUDGET_MS = 120_000;
const FORTY_DROP_BUDGET_MS = 600_000;
const FORTY = 40;
const CLEARED_LEVELS = 5;
const HOUR = 3_600_000;

/** The drop pass's total: rows by estimate, keys by measurement. */
async function total(d: ScaleDev) {
	const sizes = await getMeta(d.db, 'sizes');
	const keys = Object.values(sizes).reduce((n, s) => n + s.key_bytes, 0);
	return (await d.count('quiz_questions')) * ROW_BYTES + keys;
}

describe('the drop pass at 300,000 questions', () => {
	it('drops a cascade that left the window, leaving its cascade, quiz and attempt rows', async () => {
		const server = new FakeServer();
		const { cascade, source } = ids(1);
		server.create({ id: cascade, source_id: source, count: QUESTIONS });
		const d = await ScaleDev.make(server);
		await d.open(cascade);
		await d.sync();
		expect(await d.countFor('quiz_questions', cascade)).toBe(QUESTIONS);
		// The automatic keep turned off, and fifteen days on.
		await setKeepOffline(d.db, cascade, false, null, new Date(d.now));
		d.now += 15 * DAY;
		const [, ms] = await timed(async () => d.dl.dropPass(await policy(d.db, new Date(d.now))));
		report('drop pass, one cascade out of the window', ms, ONE_DROP_BUDGET_MS, `${QUESTIONS} rows, keys and answers`);
		for (const s of ['quiz_questions', 'questions', 'cards'] as const) expect(await d.countFor(s, cascade)).toBe(0);
		expect(await d.db.get('cascades', cascade)).toBeDefined();
		expect(await d.db.get('quizzes', source)).toBeDefined();
		expect(ms).toBeLessThanOrEqual(ONE_DROP_BUDGET_MS);
	});

	it('brings forty automatically kept cascades under ROW_STORAGE_BUDGET, oldest first, and nothing is refetched', async () => {
		const server = new FakeServer();
		const all = Array.from({ length: FORTY }, (_, i) => ids(i + 1));
		for (const { cascade, source } of all) server.create({ id: cascade, source_id: source, count: QUESTIONS });
		const d = await ScaleDev.make(server);
		// Answers are beside the point here, and forty cascades' would pass the soft limit.
		d.refuse = (c) => c.kind === 'cards';
		const t0 = d.now;
		// Opened an hour apart, the first oldest; the first is also kept by hand.
		for (let i = 0; i < FORTY; i++) {
			d.now = t0 - (FORTY - i) * HOUR;
			await d.open(all[i].cascade);
		}
		d.now = t0;
		await setKeepOffline(d.db, all[0].cascade, true, null, new Date(t0 - FORTY * HOUR));
		await d.engine.sync();

		// The second-oldest also holds five cleared 150,000-question levels.
		const heavy = all[1];
		const other = new ServerPlayer(server);
		for (let l = 0; l < CLEARED_LEVELS; l++) {
			const level2 = `b0000000-0000-4000-8000-${l.toString(16).padStart(12, '0')}`;
			other.grade(heavy.cascade, heavy.source, (p) => p % 2 === 1);
			other.finish(heavy.cascade, heavy.source, BigInt(100 + l), level2);
			other.grade(heavy.cascade, level2, () => false);
			await d.engine.sync();
			await d.dl.materialiseQuiz((await d.db.get('cascades', heavy.cascade))!, (await d.db.get('quizzes', level2))!);
			other.finish(heavy.cascade, level2, BigInt(200 + l), `e0000000-0000-4000-8000-${l.toString(16).padStart(12, '0')}`);
			await d.engine.sync();
		}
		// Everything downloaded: every cascade's rows and keys.
		await d.dl.run().catch(() => undefined);
		const pol = await policy(d.db, new Date(d.now));
		expect(pol.autoKept.size).toBe(FORTY - 1);
		for (const { cascade } of all) expect(await d.countFor('questions', cascade)).toBe(QUESTIONS);
		const heavyRows = await d.countFor('quiz_questions', heavy.cascade);
		expect(heavyRows).toBe(QUESTIONS + CLEARED_LEVELS * (QUESTIONS / 2));
		const sizesBefore = await getMeta(d.db, 'sizes');
		const sizeOf = new Map<string, number>();
		for (const { cascade } of all) sizeOf.set(cascade, (await d.countFor('quiz_questions', cascade)) * ROW_BYTES + (sizesBefore[cascade]?.key_bytes ?? 0));
		const over = await total(d);
		report('before the pass', 0, null, `${await d.count('quiz_questions')} rows, ${(over / 2 ** 30).toFixed(2)} GiB of ${(ROW_STORAGE_BUDGET / 2 ** 30).toFixed(0)} GiB`);
		expect(over).toBeGreaterThan(ROW_STORAGE_BUDGET);
		expect(QUESTIONS).toBeGreaterThan(AUTO_KEEP_OFFLINE_ROWS);

		const [, ms] = await timed(async () => d.dl.dropPass(await policy(d.db, new Date(d.now))));
		const dropped = await getMeta(d.db, 'budget_dropped');
		const after = await total(d);
		report('drop pass, forty cascades', ms, FORTY_DROP_BUDGET_MS, `${dropped.length} dropped, ${(after / 2 ** 30).toFixed(2)} GiB left`);
		expect(after).toBeLessThanOrEqual(ROW_STORAGE_BUDGET);
		// Oldest last-open first, skipping the one kept by hand, and no more than needed:
		// the heavy one, then the next-oldest, until under budget.
		expect(dropped).toEqual(all.slice(1, 1 + dropped.length).map((x) => x.cascade));
		expect(after + sizeOf.get(dropped.at(-1)!)!).toBeGreaterThan(ROW_STORAGE_BUDGET);
		// The hand-kept cascade, the oldest opened, is whole.
		expect(await d.countFor('quiz_questions', all[0].cascade)).toBe(QUESTIONS);
		expect(await d.countFor('questions', all[0].cascade)).toBe(QUESTIONS);
		// The cleared levels went with their cascade.
		expect(await d.countFor('quiz_questions', heavy.cascade)).toBe(0);
		expect(ms).toBeLessThanOrEqual(FORTY_DROP_BUDGET_MS);

		// One more pull: nothing dropped is refetched, and the total stays under.
		d.calls = [];
		const before = d.requests.length;
		await d.sync().catch(() => undefined);
		const holding = [];
		for (const { cascade } of all) if ((await d.countFor('quiz_questions', cascade)) > 0) holding.push(cascade);
		expect(d.calls.filter((c) => dropped.includes(c.cascade))).toEqual([]);
		expect(d.requests[before].question_rows_for).toEqual([...holding].sort());
		expect(holding).toContain(all[0].cascade);
		for (const id of dropped) expect(holding).not.toContain(id);
		expect(await getMeta(d.db, 'budget_dropped')).toEqual(dropped);
		expect(await total(d)).toBeLessThanOrEqual(ROW_STORAGE_BUDGET);
	});
});
