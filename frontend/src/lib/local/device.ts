// This device's id for an account: made once per device and user, so each
// account on a device syncs as its own device (PLAN.md § On the device → meta).
// Phase 7a keeps it in the per-user `meta` store; until then it lives here.
const key = (userId: string) => `wordfall-device-${userId}`;

export function deviceId(userId: string): string {
	try {
		let id = localStorage.getItem(key(userId));
		if (!id) {
			id = crypto.randomUUID();
			localStorage.setItem(key(userId), id);
		}
		return id;
	} catch {
		return crypto.randomUUID();
	}
}
