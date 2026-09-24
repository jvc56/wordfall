// PLAN.md § Scale tests: "assert the keys-only pass for ten such cascades
// takes 30 card-page requests in all, a tenth of the 300 their answers need,
// and that the bytes it moves are within a quarter of those ten cascades'
// answers rather than a tenth, the ratio Downloads states."
import { describe, expect, it } from 'vitest';
import { CARDS_PAGE } from './downloads';
import { FakeServer } from './testing/fake-server';
import { ids, QUESTIONS, report, ScaleDev, timed } from './testing/scale';

const CASCADES = 10;
const KEYS_PASS_BUDGET_MS = 1_200_000;

describe('the keys-only pass for ten 300,000-question cascades', () => {
	it('takes 30 requests, a tenth of the answers’ 300, moving under a quarter of their bytes', async () => {
		const server = new FakeServer();
		const d = await ScaleDev.make(server);
		const all = Array.from({ length: CASCADES }, (_, i) => ids(i + 1));
		for (const { cascade, source } of all) server.create({ id: cascade, source_id: source, count: QUESTIONS });
		await d.open(...all.map((x) => x.cascade));
		// Stop the pass where the answers would begin: the keys are the whole of it.
		d.refuse = (c) => c.kind === 'cards';
		const [, ms] = await timed(async () => {
			await d.engine.sync();
			await d.dl.run().catch(() => undefined);
		});
		const keys = d.calls.filter((c) => c.kind === 'keys');
		const keyBytes = keys.reduce((n, c) => n + c.bytes, 0);
		for (const { cascade } of all) expect(await d.countFor('questions', cascade)).toBe(QUESTIONS);
		// The answers those ten cascades would need: 300 pages, weighed without storing them.
		let answerPages = 0;
		let answerBytes = 0;
		for (const { cascade } of all) {
			for (let from = 0; from < QUESTIONS; from += CARDS_PAGE) {
				answerBytes += JSON.stringify(server.cards(cascade, from, CARDS_PAGE, false, false)).length;
				answerPages++;
			}
		}
		report(
			'keys-only pass, ten cascades',
			ms,
			KEYS_PASS_BUDGET_MS,
			`${keys.length} requests, ${(keyBytes / 1e6).toFixed(1)} MB; the answers: ${answerPages} requests, ${(answerBytes / 1e6).toFixed(1)} MB; ratio ${(keyBytes / answerBytes).toFixed(3)}`
		);
		expect(keys.length).toBe(30);
		expect(answerPages).toBe(300);
		expect(keyBytes).toBeLessThanOrEqual(answerBytes / 4);
		expect(ms).toBeLessThanOrEqual(KEYS_PASS_BUDGET_MS);
	});
});
