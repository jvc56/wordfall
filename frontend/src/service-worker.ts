// The app shell (PLAN.md § On the device → App shell). Precaches the built app
// and answers every navigation to an app route with the cached index.html, so
// /cascades/:id loads with no connection. A new version installs and waits:
// it never calls skipWaiting() on install, only when a page posts
// SKIP_WAITING, and it deletes an old build's cache only once no page runs
// that build.
/// <reference types="@sveltejs/kit" />
/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />
import { build, files, version } from '$service-worker';
import { cacheName, CACHE_PREFIX, deletable, route, type ToWorker } from '$lib/sw/logic';

const sw = self as unknown as ServiceWorkerGlobalScope;
const CACHE = cacheName(version);
const SHELL = '/index.html';
const precached = new Set([...build, ...files, SHELL]);
/** The build each page reports it runs, by client id. */
const clientVersions = new Map<string, string>();

sw.addEventListener('install', (event) => {
	event.waitUntil(caches.open(CACHE).then((c) => c.addAll([...precached])));
});

async function prune() {
	const live = new Set((await sw.clients.matchAll({ type: 'window', includeUncontrolled: true })).map((c) => c.id));
	for (const id of [...clientVersions.keys()]) if (!live.has(id)) clientVersions.delete(id);
	const names = (await caches.keys()).filter((n) => n.startsWith(CACHE_PREFIX));
	for (const n of deletable(names, version, clientVersions.values())) await caches.delete(n);
}

sw.addEventListener('activate', (event) => {
	event.waitUntil(sw.clients.claim().then(prune));
});

sw.addEventListener('message', (event) => {
	const m = event.data as ToWorker;
	const id = (event.source as Client | null)?.id;
	if (m.type === 'SKIP_WAITING') void sw.skipWaiting();
	else if (m.type === 'HELLO' && id) clientVersions.set(id, m.version);
	else if (m.type === 'CLOSING') {
		if (id) clientVersions.delete(id);
		event.waitUntil(prune());
	}
});

sw.addEventListener('fetch', (event) => {
	const req = event.request;
	if (req.method !== 'GET') return;
	const url = new URL(req.url);
	const r = route(url, req.mode, sw.location.origin, precached);
	if (r === 'network') return; // untouched: /api, the export frame, anything unrecognised
	event.respondWith(
		(async () => {
			const cache = await caches.open(CACHE);
			const hit = await cache.match(r === 'shell' ? SHELL : url.pathname);
			if (hit) return hit;
			// An old build's chunk, for a tab still running it.
			const old = await caches.match(r === 'shell' ? SHELL : url.pathname);
			return old ?? fetch(req);
		})()
	);
});
