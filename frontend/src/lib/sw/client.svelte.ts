// The page's side of updates (PLAN.md § On the device → App shell). The app
// watches `registration.waiting` and offers "a new version is ready"; when the
// user accepts — or presses reload in the 426 state — it posts SKIP_WAITING.
// Only the tab that posted it reloads on `controllerchange`; every other tab
// shows "Wordfall was updated in another tab — reload to continue" and keeps
// running. The app never reloads on its own initiative.
import { version } from '$app/environment';
import type { ToWorker } from './logic';

class UpdateState {
	/** A new version installed and is waiting. */
	ready = $state(false);
	/** Another tab activated a new version under this one. */
	updatedElsewhere = $state(false);
}

export const updates = new UpdateState();

let asked = false;
let registration: ServiceWorkerRegistration | null = null;

function post(to: ServiceWorker | null | undefined, m: ToWorker) {
	to?.postMessage(m);
}

export async function watchUpdates() {
	if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) return;
	const sw = navigator.serviceWorker;
	// The build this page runs, for support and for the update journey's check.
	document.documentElement.dataset.build = String(typeof __APP_BUILD__ === 'number' ? __APP_BUILD__ : 0);
	registration = (await sw.getRegistration()) ?? null;
	// A card can stay open for hours with no navigation to prompt the browser's
	// own check, so look for a new version now and then, and on returning.
	const lookForUpdate = () => void registration?.update().catch(() => undefined);
	setInterval(lookForUpdate, 5 * 60_000);
	addEventListener('online', lookForUpdate);
	document.addEventListener('visibilitychange', () => {
		if (document.visibilityState === 'visible') lookForUpdate();
	});
	post(sw.controller, { type: 'HELLO', version });
	addEventListener('pagehide', () => post(sw.controller, { type: 'CLOSING', version }));
	sw.addEventListener('controllerchange', () => {
		if (asked) location.reload();
		else updates.updatedElsewhere = true;
	});
	const check = (r: ServiceWorkerRegistration) => {
		if (r.waiting) updates.ready = true;
		r.addEventListener('updatefound', () => {
			const w = r.installing;
			w?.addEventListener('statechange', () => {
				if (w.state === 'installed' && sw.controller) updates.ready = true;
			});
		});
	};
	if (registration) check(registration);
}

/** The user accepted the new version: the one reload they asked for. */
export function applyUpdate() {
	const waiting = registration?.waiting;
	if (!waiting) {
		location.reload();
		return;
	}
	asked = true;
	post(waiting, { type: 'SKIP_WAITING' });
}
