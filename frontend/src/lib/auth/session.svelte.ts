// The signed-in account for this tab (PLAN.md § Authentication while offline).
// The unscoped `signed_in` pointer is read before anything else at startup,
// and the user is treated as signed in until a sync is answered 401.
import { ApiError, bindUser, readCookie, CSRF_COOKIE, CSRF_HEADER } from '$lib/api';
import {
	getAccount,
	recordLogin,
	recordLogout,
	retryQueuedLogout,
	signedIn,
	SIGNED_IN_CHANNEL,
	type LogoutOutcome
} from '$lib/local/accounts';
import { closeUserDb, onUpdatedElsewhere, userDb } from '$lib/local/open';
import { authApi } from './client';
import type { Me } from './types';

class Session {
	ready = $state(false);
	userId = $state<string | null>(null);
	username = $state('');
	isAdmin = $state(false);
	/** Another tab signed in as someone else: this tab stops syncing and keeps its data. */
	signedOutInAnotherTab = $state(false);
	/** A newer build in another tab took the database: "reload to continue". */
	updatedInAnotherTab = $state(false);
	/** The server no longer accepts this tab's session: "Log in to sync". */
	needsLogin = $state(false);
	me = $state<Me | null>(null);
	/** Signing out on the way to the landing page (a deleted account), not to /login. */
	toLanding = $state(false);
}

export const session = new Session();

/** The queued logout's request, with the CSRF token whenever the cookie is there. */
export async function sendQueuedLogout(): Promise<LogoutOutcome> {
	const headers: Record<string, string> = {};
	const csrf = readCookie(CSRF_COOKIE);
	if (csrf) headers[CSRF_HEADER] = csrf;
	try {
		const r = await fetch('/api/auth/logout', {
			method: 'POST',
			headers,
			credentials: 'same-origin'
		});
		return { status: r.status };
	} catch {
		return { networkError: true };
	}
}

function applyMe(me: Me) {
	session.me = me;
	session.isAdmin = me.is_admin;
	session.username = me.username;
	// The trash retention period and max_quiz_questions as /api/auth/me last reported them.
	void userDb(me.user_id, me.username)
		.then((db) =>
			db.put('meta', { trash_retention_days: me.trash_retention_days, max_quiz_questions: me.max_quiz_questions }, 'server')
		)
		.catch(() => undefined);
}

onUpdatedElsewhere(() => {
	session.updatedInAnotherTab = true;
});

let channel: BroadcastChannel | null = null;
let onlineHooked = false;

export async function initSession() {
	const id = await signedIn();
	if (id) {
		const row = await getAccount(id);
		session.userId = id;
		session.username = row?.username ?? '';
		bindUser(id);
		void userDb(id, session.username).catch(() => undefined);
	} else {
		await retryQueuedLogout(sendQueuedLogout);
	}
	session.ready = true;

	// A queued logout is retried at the next connection on which no account is signed in.
	if (!onlineHooked) {
		onlineHooked = true;
		addEventListener('online', () => {
			if (!session.userId) void retryQueuedLogout(sendQueuedLogout);
		});
	}

	if (typeof BroadcastChannel !== 'undefined' && !channel) {
		channel = new BroadcastChannel(SIGNED_IN_CHANNEL);
		channel.onmessage = (ev) => {
			const other = (ev.data as { signed_in: string | null }).signed_in;
			if (session.userId) session.signedOutInAnotherTab = other !== session.userId;
		};
	}

	if (id && navigator.onLine) {
		try {
			const me = await authApi.me();
			if (me.user_id !== id) session.needsLogin = true;
			else applyMe(me);
		} catch (e) {
			if (e instanceof ApiError && e.status === 401) session.needsLogin = true;
		}
	}
}

export async function login(username: string, password: string): Promise<Me> {
	const me = await authApi.login(username, password);
	await recordLogin(me.user_id, me.username);
	session.userId = me.user_id;
	session.needsLogin = false;
	session.signedOutInAnotherTab = false;
	await userDb(me.user_id, me.username);
	applyMe(me);
	bindUser(me.user_id);
	// PLAN.md § On the device: ask the browser to keep this site's data.
	void navigator.storage?.persist?.().catch(() => false);
	return me;
}

/** Always completes locally; the server request is queued and retried. */
export async function logout() {
	const id = session.userId;
	if (id) await recordLogout(id);
	closeUserDb();
	session.userId = null;
	session.me = null;
	session.isAdmin = false;
	session.username = '';
	bindUser(null);
	await retryQueuedLogout(sendQueuedLogout);
}

/** After DELETE /api/account succeeded: the session is already void. */
export function forgetSession() {
	closeUserDb();
	session.userId = null;
	session.me = null;
	session.isAdmin = false;
	session.username = '';
	bindUser(null);
}
