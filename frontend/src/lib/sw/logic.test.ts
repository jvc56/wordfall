// PLAN.md § On the device → App shell: navigations to app routes get the
// cached shell; /api, the export frame and unrecognised requests go to the
// network untouched; an old build's cache goes only once no page runs it.
import { describe, expect, it } from 'vitest';
import { cacheName, deletable, route } from './logic';

const O = 'https://wordfall.example';
const pre = new Set(['/_app/immutable/entry/start.js', '/favicon.svg', '/index.html']);
const u = (p: string) => new URL(p, O);

describe('the service worker', () => {
	it('answers every navigation to an app route with the shell', () => {
		for (const p of ['/', '/cascades', '/cascades/5f0c0c1e-0000-4000-8000-000000000000', '/trash', '/account']) {
			expect(route(u(p), 'navigate', O, pre)).toBe('shell');
		}
	});

	it('sends /api navigations, the export frame included, and /api requests to the network', () => {
		expect(route(u('/api/cascades/x/export?format=csv'), 'navigate', O, pre)).toBe('network');
		expect(route(u('/api/sync'), 'cors', O, pre)).toBe('network');
		expect(route(u('/api/auth/me'), 'same-origin', O, pre)).toBe('network');
	});

	it('serves the built app from the cache and leaves anything unrecognised alone', () => {
		expect(route(u('/_app/immutable/entry/start.js'), 'no-cors', O, pre)).toBe('precached');
		expect(route(u('/_app/immutable/chunks/unknown.js'), 'no-cors', O, pre)).toBe('network');
		expect(route(new URL('https://fonts.example/x.woff2'), 'cors', O, pre)).toBe('network');
	});

	it('deletes an old build’s cache only once no page still runs that build', () => {
		const names = [cacheName('1'), cacheName('2'), cacheName('3'), 'other-cache'];
		expect(deletable(names, '3', ['2'])).toEqual([cacheName('1')]);
		expect(deletable(names, '3', [])).toEqual([cacheName('1'), cacheName('2')]);
		expect(deletable(names, '3', ['3'])).toEqual([cacheName('1'), cacheName('2')]);
	});
});
