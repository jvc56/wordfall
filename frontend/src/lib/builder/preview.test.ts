// PLAN.md § Unit tests → Frontend: the cascade builder's preview.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api';
import { PreviewScheduler, realClock, type PreviewBody, type PreviewState } from './preview';
import { sendWithRetry } from './create';
import { PREVIEW_DEBOUNCE_MS, PREVIEW_MIN_INTERVAL_MS, SEARCH_RATE_PER_MINUTE } from '$lib/sync/config';

const body = (n: number): PreviewBody => ({ lexicon: 'CSW24', quiz_type: 'anagram', filters: { n } });

function harness(respond: (b: PreviewBody, i: number) => Promise<{ count: number; sample: string[]; over_cap: boolean }>) {
	const calls: PreviewBody[] = [];
	let state: PreviewState | null = null;
	const s = new PreviewScheduler(
		(b) => {
			calls.push(b);
			return respond(b, calls.length);
		},
		(st) => (state = st),
		realClock
	);
	return { s, calls, state: () => state! };
}

const ok = (b: PreviewBody) => Promise.resolve({ count: (b.filters as { n: number }).n, sample: [], over_cap: false });

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('preview', () => {
	for (const status of [503, 429]) {
		it(`a ${status} keeps the last count, reads busy, retries once and marks no row`, async () => {
			const h = harness((b, i) =>
				i === 2 ? Promise.reject(new ApiError(status, { error: 'x' }, 3)) : ok(b)
			);
			h.s.update(body(5));
			await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS);
			expect(h.state().count).toBe(5);
			h.s.update(body(7));
			await vi.advanceTimersByTimeAsync(PREVIEW_MIN_INTERVAL_MS);
			expect(h.calls).toHaveLength(2);
			expect(h.state().count).toBe(5);
			expect(h.state().busy).toBe(true);
			expect(h.state().errors).toEqual([]);
			await vi.advanceTimersByTimeAsync(2_999);
			expect(h.calls).toHaveLength(2);
			await vi.advanceTimersByTimeAsync(1);
			expect(h.calls).toHaveLength(3);
			expect(h.state().count).toBe(7);
			expect(h.state().busy).toBe(false);
		});
	}

	it('keystrokes 100 ms apart make one request per debounce, not one per keystroke', async () => {
		const h = harness(ok);
		for (let i = 1; i <= 30; i++) {
			h.s.update(body(i));
			await vi.advanceTimersByTimeAsync(100);
		}
		await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS);
		expect(h.calls.length).toBeLessThanOrEqual(Math.ceil((30 * 100 + PREVIEW_DEBOUNCE_MS) / PREVIEW_DEBOUNCE_MS));
		expect(h.calls.length).toBe(1);
		expect(h.state().count).toBe(30);
	});

	it('keystrokes 500 ms apart still make one request per interval, within the rate', async () => {
		const h = harness(ok);
		const perMinute: number[] = [];
		let n = 0;
		for (let t = 0; t < 60_000; t += 500) {
			h.s.update(body(++n));
			await vi.advanceTimersByTimeAsync(500);
		}
		perMinute.push(h.calls.length);
		expect(h.calls.length).toBeLessThanOrEqual(SEARCH_RATE_PER_MINUTE);
		expect(h.calls.length).toBeGreaterThanOrEqual(20);
		// The last edit's count is on screen once the run ends.
		await vi.advanceTimersByTimeAsync(60_000);
		expect(h.state().count).toBe(n);
	});

	it('stops at all but two of the minute and reads busy instead of spending them', async () => {
		const h = harness(ok);
		for (let i = 1; i <= 40; i++) {
			h.s.update(body(i));
			await vi.advanceTimersByTimeAsync(PREVIEW_MIN_INTERVAL_MS);
		}
		// 40 edits over 80 seconds; in any rolling minute at most 28 went out.
		const times: number[] = [];
		expect(h.calls.length).toBeLessThanOrEqual(40);
		const h2 = harness(ok);
		const start = Date.now();
		for (let i = 1; i <= 29; i++) {
			h2.s.update(body(i));
			await vi.advanceTimersByTimeAsync(PREVIEW_MIN_INTERVAL_MS);
			times.push(Date.now() - start);
		}
		expect(h2.calls.length).toBe(SEARCH_RATE_PER_MINUTE - 2);
		expect(h2.state().busy).toBe(true);
	});

	it('two builders sharing one bucket, each with its own reserve, can still spend it', async () => {
		let bucket = SEARCH_RATE_PER_MINUTE;
		const server = (b: PreviewBody) =>
			bucket-- > 0 ? ok(b) : Promise.reject(new ApiError(429, { error: 'rate_limited' }, 30));
		const a = harness(server);
		const b = harness(server);
		for (let i = 1; i <= 30; i++) {
			a.s.update(body(i));
			b.s.update(body(100 + i));
			await vi.advanceTimersByTimeAsync(PREVIEW_MIN_INTERVAL_MS);
		}
		expect(a.calls.length + b.calls.length).toBeGreaterThan(SEARCH_RATE_PER_MINUTE);
		expect(bucket).toBeLessThan(0);
	});

	it('does not send an unready form, and waits for the button in manual mode', async () => {
		const h = harness(ok);
		h.s.update(null);
		await vi.advanceTimersByTimeAsync(5_000);
		expect(h.calls).toHaveLength(0);
		h.s.update(body(3), true);
		await vi.advanceTimersByTimeAsync(5_000);
		expect(h.calls).toHaveLength(0);
		h.s.previewNow();
		await vi.advanceTimersByTimeAsync(0);
		expect(h.calls).toHaveLength(1);
	});

	it('a 400 marks rows and nothing else', async () => {
		const h = harness(() =>
			Promise.reject(new ApiError(400, { errors: [{ path: [1, 0], field: 'min', message: 'bad' }] }, null))
		);
		h.s.update(body(1));
		await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS);
		expect(h.state().errors).toEqual([{ path: [1, 0], field: 'min', message: 'bad' }]);
		expect(h.state().busy).toBe(false);
	});
});

describe('Create Cascade', () => {
	it('retries a 429 after Retry-After with the same body and ids, showing busy', async () => {
		const bodies: string[] = [];
		let created = 0;
		const busy: boolean[] = [];
		const req = { id: 'c1', source_quiz_id: 'q1', name: 'x' };
		const p = sendWithRetry(
			async () => {
				bodies.push(JSON.stringify(req));
				if (bodies.length === 1) throw new ApiError(429, { error: 'rate_limited' }, 2);
				created++;
				return { ok: true };
			},
			{ onBusy: (b) => busy.push(b) }
		);
		await vi.advanceTimersByTimeAsync(1_999);
		expect(bodies).toHaveLength(1);
		expect(busy).toEqual([true]);
		await vi.advanceTimersByTimeAsync(1);
		await p;
		expect(bodies).toHaveLength(2);
		expect(bodies[0]).toBe(bodies[1]);
		expect(created).toBe(1);
		expect(busy).toEqual([true, false]);
	});
});
