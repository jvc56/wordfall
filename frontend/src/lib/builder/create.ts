// Create Cascade, Start over and Save Search… retry a 5xx, a 429 (after
// Retry-After) or a lost connection with the same body, which their
// device-minted ids make safe; the button reads "the server is busy" meanwhile
// (PLAN.md § Creating a cascade, § Authentication → Security generally).
import { ApiError } from '$lib/api';

export interface RetryOptions {
	onBusy?: (busy: boolean) => void;
	sleep?: (ms: number) => Promise<void>;
	maxAttempts?: number;
}

const defaultSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function isRetriable(e: unknown): boolean {
	if (e instanceof ApiError) return e.status === 429 || e.status >= 500;
	return e instanceof TypeError; // fetch's network failure
}

export async function sendWithRetry<T>(send: () => Promise<T>, opts: RetryOptions = {}): Promise<T> {
	const sleep = opts.sleep ?? defaultSleep;
	const max = opts.maxAttempts ?? 20;
	for (let attempt = 1; ; attempt++) {
		try {
			const r = await send();
			opts.onBusy?.(false);
			return r;
		} catch (e) {
			if (!isRetriable(e) || attempt >= max) {
				opts.onBusy?.(false);
				throw e;
			}
			opts.onBusy?.(true);
			const s = e instanceof ApiError && e.retryAfterSeconds != null ? e.retryAfterSeconds : Math.min(2 ** attempt, 30);
			await sleep(s * 1000);
		}
	}
}
