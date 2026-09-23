// The running sync engine for the signed-in account (PLAN.md § Frontend →
// lib/sync). One tab per account leads, elected with Web Locks; the others
// read the shared stores, show the leader's status, and ask it to sync after
// their own writes. A 401 shows "Log in to sync" and studying continues.
import { session } from '$lib/auth/session.svelte';
import { recordTotals } from '$lib/local/accounts';
import type { UserDb } from '$lib/local/db';
import { userDb } from '$lib/local/open';
import { DownloadManager, openPlayers } from './downloads';
import { electLeader, startTriggers, SyncEngine, type SyncStatus } from './engine';
import type { Notice } from './notices';

class SyncState {
	status = $state<SyncStatus>('idle');
	notices = $state<Notice[]>([]);
	leader = $state(false);
	/** Bumped whenever the download manager makes progress; pages re-read their badges. */
	progress = $state(0);
	/** The cascade this tab's player has open (the drop pass skips it). */
	openCascade = $state<string | null>(null);
	overBudgetByUserKept = $state(false);
	/** Cascades a rebase changed: a player showing one refreshes (step 5). */
	changed = $state<{ ids: string[]; tick: number }>({ ids: [], tick: 0 });
}

export const syncState = new SyncState();

let engine: SyncEngine | null = null;
let downloads: DownloadManager | null = null;

/** The running download manager, in the leader tab. */
export function downloadManager(): DownloadManager | null {
	return downloads;
}

/** The card page the player needs now, in whichever tab it is. */
export async function fetchCard(db: UserDb, cascadeId: string, idx: number) {
	if (typeof navigator !== 'undefined' && !navigator.onLine) throw new Error('offline');
	await (downloads ?? new DownloadManager(db)).fetchForPlayer(cascadeId, idx);
}

/** A first open's fetch sequence, in whichever tab the player is. */
export async function ensureCascade(db: UserDb, cascadeId: string) {
	const dm =
		downloads ??
		new DownloadManager(db, {
			sync: async () => {
				afterLocalWrite();
			}
		});
	await dm.ensureCascade(cascadeId);
}
let channel: BroadcastChannel | null = null;
let stop: (() => void) | null = null;

type Message =
	| { kind: 'status'; status: SyncStatus }
	| { kind: 'kick' }
	| { kind: 'notices'; notices: Notice[] }
	| { kind: 'changed'; ids: string[] }
	| { kind: 'progress' };

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
			else if (m.kind === 'changed') syncState.changed = { ids: m.ids, tick: syncState.changed.tick + 1 };
			else if (m.kind === 'progress') syncState.progress += 1;
		};
	}
	const release = electLeader(userId, () => {
		let stopTriggers: (() => void) | null = null;
		let cancelled = false;
		syncState.leader = true;
		void userDb(userId).then((db) => {
			if (cancelled) return;
			const e = new SyncEngine(db, {
				// The download manager's turn after every pull; never awaited by the
				// cycle, since its grades fetch syncs first.
				onSynced: (o) => {
					if (o.changed.size) {
						const ids = [...o.changed];
						syncState.changed = { ids, tick: syncState.changed.tick + 1 };
						post({ kind: 'changed', ids });
					}
					void downloads
						?.run()
						.then(() => post({ kind: 'progress' }))
						.catch(() => undefined);
				},
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
			engine = e;
			downloads = new DownloadManager(db, {
				sync: () => e.sync(),
				openElsewhere: openPlayers,
				openHere: () => syncState.openCascade,
				onProgress: () => (syncState.progress += 1),
				onTotals: (t) => {
					syncState.overBudgetByUserKept = t.over_budget_by_user_kept;
					void recordTotals(userId, { rows: t.rows, keys: t.keys, answer_bytes: t.answer_bytes });
				}
			});
			stopTriggers = startTriggers(e);
		});
		return () => {
			cancelled = true;
			stopTriggers?.();
			engine = null;
			downloads = null;
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
