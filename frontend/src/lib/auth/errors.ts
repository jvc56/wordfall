import { ApiError } from '$lib/api';

/** A sentence for a failed auth request that has no field errors. */
export function describe(e: unknown): string {
	if (e instanceof ApiError) {
		if (e.status === 429)
			return `Too many attempts. Try again in ${e.retryAfterSeconds ?? 60} seconds.`;
		if (e.status >= 500) return 'Something went wrong on the server. Try again shortly.';
		return 'That did not work. Check the details and try again.';
	}
	return 'Could not reach Wordfall. Check your connection.';
}
