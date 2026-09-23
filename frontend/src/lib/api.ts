// The one HTTP client for /api (PLAN.md § API, § Authentication).
//
// - JSON bodies are sent as application/json (the server answers 415 otherwise).
// - Every cookie-authenticated write double-submits the CSRF cookie's token.
// - Every authenticated request names the account the tab runs as in
//   X-Wordfall-User (logout and GET /api/auth/me are exempt server-side).
// - 429 and 503 carry Retry-After, which every caller honours.

export const CSRF_COOKIE = 'wordfall_csrf'; // PQ-002
export const CSRF_HEADER = 'X-CSRF-Token'; // PQ-002
export const USER_HEADER = 'X-Wordfall-User';

export interface FieldError {
	field: string;
	message: string;
}

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: unknown,
		public retryAfterSeconds: number | null
	) {
		super(`HTTP ${status}`);
	}

	get fieldErrors(): FieldError[] {
		const b = this.body as { errors?: FieldError[] } | null;
		return Array.isArray(b?.errors) ? b.errors : [];
	}

	get code(): string | undefined {
		return (this.body as { error?: string } | null)?.error;
	}
}

export function readCookie(name: string): string | null {
	if (typeof document === 'undefined') return null;
	for (const part of document.cookie.split(';')) {
		const [k, ...v] = part.trim().split('=');
		if (k === name) return decodeURIComponent(v.join('='));
	}
	return null;
}

let boundUserId: string | null = null;

/** The account this tab runs as; sent as X-Wordfall-User. */
export function bindUser(userId: string | null) {
	boundUserId = userId;
}

export function boundUser(): string | null {
	return boundUserId;
}

export interface RequestOptions {
	body?: unknown;
	/** Send X-Wordfall-User (default true when a user is bound). */
	bindUser?: boolean;
	signal?: AbortSignal;
	headers?: Record<string, string>;
}

function retryAfter(resp: Response): number | null {
	const v = resp.headers.get('Retry-After');
	if (!v) return null;
	const n = Number(v);
	return Number.isFinite(n) ? n : null;
}

export async function request<T = unknown>(
	method: string,
	path: string,
	opts: RequestOptions = {}
): Promise<T> {
	const headers: Record<string, string> = { ...(opts.headers ?? {}) };
	let body: BodyInit | undefined;
	if (opts.body !== undefined) {
		headers['Content-Type'] = 'application/json';
		body = JSON.stringify(opts.body);
	}
	if (method !== 'GET' && method !== 'HEAD') {
		const csrf = readCookie(CSRF_COOKIE);
		if (csrf) headers[CSRF_HEADER] = csrf;
	}
	if ((opts.bindUser ?? true) && boundUserId) headers[USER_HEADER] = boundUserId;
	const resp = await fetch(path, {
		method,
		headers,
		body,
		credentials: 'same-origin',
		signal: opts.signal
	});
	const text = await resp.text();
	let parsed: unknown = null;
	if (text) {
		try {
			parsed = JSON.parse(text);
		} catch {
			parsed = text;
		}
	}
	if (!resp.ok) throw new ApiError(resp.status, parsed, retryAfter(resp));
	return parsed as T;
}

export const api = {
	get: <T>(path: string, opts?: RequestOptions) => request<T>('GET', path, opts),
	post: <T>(path: string, body?: unknown, opts?: RequestOptions) =>
		request<T>('POST', path, { ...opts, body }),
	delete: <T>(path: string, body?: unknown, opts?: RequestOptions) =>
		request<T>('DELETE', path, { ...opts, body })
};

/**
 * Retries a request after 429 or 5xx, waiting out Retry-After, with the same
 * body. For Create Cascade, Start over and Save Search…, whose device-minted
 * ids make a retry safe (PLAN.md § Authentication → Security generally).
 */
export async function withRetry<T>(
	fn: () => Promise<T>,
	opts: { attempts?: number; signal?: AbortSignal } = {}
): Promise<T> {
	const attempts = opts.attempts ?? 6;
	for (let i = 0; ; i++) {
		try {
			return await fn();
		} catch (e) {
			const retriable =
				e instanceof ApiError ? e.status === 429 || e.status >= 500 : e instanceof TypeError;
			if (!retriable || i + 1 >= attempts || opts.signal?.aborted) throw e;
			const waitS =
				e instanceof ApiError && e.retryAfterSeconds != null
					? e.retryAfterSeconds
					: Math.min(2 ** i, 30);
			await new Promise((r) => setTimeout(r, waitS * 1000));
		}
	}
}
