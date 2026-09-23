// The running sync engine for the signed-in account (PLAN.md § Frontend →
// lib/sync). One tab per account leads, elected with Web Locks; the others
// read the shared stores, show the leader's status, and ask it to sync after
// their own writes. A 401 shows "Log in to sync" and studying continues.
import { session } from '$lib/auth/session.svelte';
import { userDb } from '$lib/local/open';
import { electLeader, startTriggers, SyncEngine, type SyncStatus } from './engine';
import type { Notice } from './notices';

class SyncState {
	status = $state<SyncStatus>('idle');
	notices = $state<Notice[]>([]);
	leader = $state(false);
}

export const syncState = new SyncState();

let engine: SyncEngine | null = null;
let channel: BroadcastChannel | null = null;
let stop: (() => void) | null = null;

type Message = { kind: 'status'; status: SyncStatus } | { kind: 'kick' } | { kind: 'notices'; notices: Notice[] };

function post(m: Message) {
	channel?.postMessage(m);
}

/** Starts syncing for this account; returns the function that stops it. */
export function startSync(userId: string): () => void {
	stopSync();
	if (typeof BroadcastChannel !== 'undefined') {
		channel = new BroadcastChannel(`wordfall-sync-${userId}`);
		channel.onmessage = (ev) => {
			const m = ev.data as Message;
			if (m.kind === 'kick') engine?.schedule();
			else if (m.kind === 'status' && !syncState.leader) syncState.status = m.status;
			else if (m.kind === 'notices' && !syncState.leader) syncState.notices = [...syncState.notices, ...m.notices];
		};
	}
	const release = electLeader(userId, () => {
		let stopTriggers: (() => void) | null = null;
		let cancelled = false;
		syncState.leader = true;
		void userDb(userId).then((db) => {
			if (cancelled) return;
			engine = new SyncEngine(db, {
				onStatus: (s) => {
					syncState.status = s;
					post({ kind: 'status', status: s });
					if (s === 'needs_login') session.needsLogin = true;
				},
				onNotices: (n) => {
					syncState.notices = [...syncState.notices, ...n];
					post({ kind: 'notices', notices: n });
				}
			});
			stopTriggers = startTriggers(engine);
		});
		return () => {
			cancelled = true;
			stopTriggers?.();
			engine = null;
			syncState.leader = false;
		};
	});
	stop = () => {
		release();
		channel?.close();
		channel = null;
	};
	return stop;
}

export function stopSync() {
	stop?.();
	stop = null;
}

/** After a local write: half a second later, by this tab or by the leader. */
export function afterLocalWrite() {
	if (engine) engine.schedule();
	else post({ kind: 'kick' });
}

export function dismissNotice(n: Notice) {
	syncState.notices = syncState.notices.filter((x) => x !== n);
}
