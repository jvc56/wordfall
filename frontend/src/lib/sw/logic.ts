// The service worker's decisions, kept pure so they can be tested (PLAN.md
// § On the device → App shell). Every navigation to an app route is answered
// with the cached index.html; a navigation under /api/ — the export is one, a
// hidden frame's — and any request the worker does not recognise as part of
// the built app go to the network untouched. /api responses are never cached.

export type Route = 'shell' | 'precached' | 'network';

export function route(url: URL, mode: RequestMode, origin: string, precached: Set<string>): Route {
	if (url.origin !== origin) return 'network';
	if (url.pathname.startsWith('/api/') || url.pathname === '/api') return 'network';
	if (mode === 'navigate') return 'shell';
	return precached.has(url.pathname) ? 'precached' : 'network';
}

export const CACHE_PREFIX = 'wordfall-';

export function cacheName(version: string): string {
	return `${CACHE_PREFIX}${version}`;
}

/**
 * The previous builds' caches that may go: one is deleted only once no page
 * is still running that build, so a tab left on the old build can still load
 * the chunks the card it is on needs.
 */
export function deletable(names: string[], current: string, clientVersions: Iterable<string>): string[] {
	const inUse = new Set([...clientVersions].map(cacheName));
	return names.filter((n) => n.startsWith(CACHE_PREFIX) && n !== cacheName(current) && !inUse.has(n));
}

/** Messages between pages and the worker. */
export type ToWorker =
	| { type: 'SKIP_WAITING' }
	/** A page announces the build it runs. */
	| { type: 'HELLO'; version: string }
	/** A page on some build is closing. */
	| { type: 'CLOSING'; version: string };
