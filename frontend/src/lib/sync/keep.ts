// Keep offline, a property of this device that never syncs (PLAN.md § On the
// device → Downloads). Turning it on counts as an open, which clears a
// budget-dropped mark, and starts the fetch at once; turning it off makes the
// cascade subject to the window again at the next drop pass, and records the
// opt-out from an automatic keep by size.
import type { UserDb } from '$lib/local/db';
import { readMeta, recordOpen, writeMeta } from '$lib/local/meta';
import type { RwTx } from '$lib/local/view';
import type { DownloadManager } from './downloads';

export async function setKeepOffline(db: UserDb, cascadeId: string, on: boolean, downloads: DownloadManager | null, now = new Date()) {
	const tx = db.transaction(['meta'], 'readwrite');
	const rtx = tx as unknown as RwTx;
	const keep = await readMeta(rtx, 'keep_offline');
	const optout = await readMeta(rtx, 'auto_keep_optout');
	if (on) {
		if (!keep.includes(cascadeId)) await writeMeta(rtx, 'keep_offline', [...keep, cascadeId]);
		if (optout.includes(cascadeId)) {
			await writeMeta(
				rtx,
				'auto_keep_optout',
				optout.filter((id) => id !== cascadeId)
			);
		}
		await recordOpen(rtx, cascadeId, now.toISOString());
	} else {
		await writeMeta(
			rtx,
			'keep_offline',
			keep.filter((id) => id !== cascadeId)
		);
		if (!optout.includes(cascadeId)) await writeMeta(rtx, 'auto_keep_optout', [...optout, cascadeId]);
	}
	await tx.done;
	if (on) await downloads?.ensureCascade(cascadeId);
	else await downloads?.run();
}
