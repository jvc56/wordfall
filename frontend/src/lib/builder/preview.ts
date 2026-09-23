// The builder's live preview (PLAN.md § Creating a cascade, step 8).
//
// - Debounced by PREVIEW_DEBOUNCE_MS, then throttled to one request every
//   PREVIEW_MIN_INTERVAL_MS, with the latest edit always sent when the
//   interval expires, so the count is never left behind the filters.
// - Keeps a reserve: it stops once this tab has spent all but two of
//   SEARCH_RATE_PER_MINUTE in the rolling minute, reading "the server is busy",
//   so Create Cascade, Start over and Save Search… still get through.
// - A 503 or a 429 is not a row error: the last count stays with "the server
//   is busy", and one retry follows after Retry-After. Only a 400 marks rows.

import { ApiError } from '$lib/api';
import {
	PREVIEW_DEBOUNCE_MS,
	PREVIEW_MIN_INTERVAL_MS,
	PREVIEW_RESERVE,
	SEARCH_RATE_PER_MINUTE
} from '$lib/sync/config';

export interface PreviewBody {
	lexicon: string;
	quiz_type: string;
	filters: unknown;
}

export interface PreviewResponse {
	count: number;
	sample: string[];
	over_cap: boolean;
}

export interface PathError {
	path: number[];
	field: string;
	message: string;
}

export interface PreviewState {
	count: number | null;
	sample: string[];
	overCap: boolean;
	busy: boolean;
	loading: boolean;
	errors: PathError[];
	failed: string | null;
}

export interface Clock {
	now(): number;
	setTimeout(fn: () => void, ms: number): unknown;
	clearTimeout(handle: unknown): void;
}

export const realClock: Clock = {
	now: () => Date.now(),
	setTimeout: (fn, ms) => setTimeout(fn, ms),
	clearTimeout: (h) => clearTimeout(h as ReturnType<typeof setTimeout>)
};

export interface PreviewOptions {
	debounceMs?: number;
	minIntervalMs?: number;
	ratePerMinute?: number;
	reserve?: number;
}

export class PreviewScheduler {
	state: PreviewState = {
		count: null,
		sample: [],
		overCap: false,
		busy: false,
		loading: false,
		errors: [],
		failed: null
	};
	/** When this tab sent each request in the last minute. */
	private sent: number[] = [];
	private latest: PreviewBody | null = null;
	private latestKey = '';
	private sentKey = '';
	private debounce: unknown = null;
	private wait: unknown = null;
	private seq = 0;
	private retried = false;
	private readonly debounceMs: number;
	private readonly minIntervalMs: number;
	private readonly budget: number;

	constructor(
		private readonly send: (body: PreviewBody) => Promise<PreviewResponse>,
		private readonly onChange: (s: PreviewState) => void,
		private readonly clock: Clock = realClock,
		opts: PreviewOptions = {}
	) {
		this.debounceMs = opts.debounceMs ?? PREVIEW_DEBOUNCE_MS;
		this.minIntervalMs = opts.minIntervalMs ?? PREVIEW_MIN_INTERVAL_MS;
		this.budget = (opts.ratePerMinute ?? SEARCH_RATE_PER_MINUTE) - (opts.reserve ?? PREVIEW_RESERVE);
	}

	private set(patch: Partial<PreviewState>) {
		this.state = { ...this.state, ...patch };
		this.onChange(this.state);
	}

	/**
	 * Every edit. `null` means the form is not ready to search (an invalid
	 * row, no lexicon). `manual` holds the request until `previewNow`.
	 */
	update(body: PreviewBody | null, manual = false) {
		this.latest = body;
		this.latestKey = body ? JSON.stringify(body) : '';
		this.retried = false;
		if (this.debounce !== null) this.clock.clearTimeout(this.debounce);
		this.debounce = null;
		if (!body || manual) return;
		this.debounce = this.clock.setTimeout(() => {
			this.debounce = null;
			this.fire();
		}, this.debounceMs);
	}

	/** The explicit Preview button. */
	previewNow() {
		if (this.debounce !== null) this.clock.clearTimeout(this.debounce);
		this.debounce = null;
		this.fire();
	}

	private prune(now: number) {
		this.sent = this.sent.filter((t) => now - t < 60_000);
	}

	/** Sends the latest edit now, or schedules it for when it may go. */
	private fire() {
		if (!this.latest || this.latestKey === this.sentKey) return;
		if (this.wait !== null) return; // already scheduled; it sends the latest
		const now = this.clock.now();
		this.prune(now);
		const last = this.sent.length ? this.sent[this.sent.length - 1] : -Infinity;
		let at = Math.max(now, last + this.minIntervalMs);
		if (this.sent.length >= this.budget) {
			// Out of this tab's share of the minute: busy until a slot frees.
			this.set({ busy: true });
			at = Math.max(at, this.sent[this.sent.length - this.budget] + 60_000);
		}
		if (at > now) {
			this.wait = this.clock.setTimeout(() => {
				this.wait = null;
				this.fire();
			}, at - now);
			return;
		}
		void this.request();
	}

	private async request() {
		const body = this.latest;
		if (!body) return;
		const key = this.latestKey;
		const seq = ++this.seq;
		this.sentKey = key;
		this.sent.push(this.clock.now());
		this.set({ loading: true });
		try {
			const r = await this.send(body);
			if (seq !== this.seq) return;
			this.set({
				count: r.count,
				sample: r.sample,
				overCap: r.over_cap,
				busy: false,
				loading: false,
				errors: [],
				failed: null
			});
		} catch (e) {
			if (seq !== this.seq) return;
			if (e instanceof ApiError && (e.status === 503 || e.status === 429)) {
				// Not a row error: keep the last count and retry once.
				this.set({ busy: true, loading: false });
				this.sentKey = '';
				if (!this.retried) {
					this.retried = true;
					const ms = (e.retryAfterSeconds ?? 1) * 1000;
					this.wait = this.clock.setTimeout(() => {
						this.wait = null;
						this.fire();
					}, ms);
				}
			} else if (e instanceof ApiError && e.status === 400) {
				const errors = ((e.body as { errors?: PathError[] })?.errors ?? []).filter((x) => Array.isArray(x.path));
				this.set({ errors, loading: false, busy: false, failed: null });
			} else if (e instanceof ApiError && e.status === 422) {
				this.set({ loading: false, busy: false, failed: 'This search is too broad to finish in time. Narrow it.' });
			} else {
				this.sentKey = '';
				this.set({ loading: false, failed: 'The preview could not reach the server.' });
			}
		}
	}

	dispose() {
		if (this.debounce !== null) this.clock.clearTimeout(this.debounce);
		if (this.wait !== null) this.clock.clearTimeout(this.wait);
		this.seq++;
	}
}
