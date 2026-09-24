// Which cascades this device holds or wants question rows for (PLAN.md
// § Downloads, § The sync cycle → Pull): those opened here in the last
// DOWNLOAD_WINDOW_DAYS and not budget-dropped, those kept offline by the user
// or automatically by size, and those whose base already holds question rows.
import type { UserDb } from '$lib/local/db';
import { getMeta } from '$lib/local/meta';
import { AUTO_KEEP_OFFLINE_ROWS, DOWNLOAD_WINDOW_DAYS } from './config';

const DAY_MS = 86_400_000;

export interface Policy {
	/** In the window or kept: its rows, keys and cards are kept downloaded. */
	wanted: Set<string>;
	/** Kept automatically by size (active rows above AUTO_KEEP_OFFLINE_ROWS, opened here, not opted out). */
	autoKept: Set<string>;
	userKept: Set<string>;
}

export async function policy(db: UserDb, now = new Date()): Promise<Policy> {
	const [opens, keep, optout, dropped] = await Promise.all([
		getMeta(db, 'opens'),
		getMeta(db, 'keep_offline'),
		getMeta(db, 'auto_keep_optout'),
		getMeta(db, 'budget_dropped')
	]);
	const userKept = new Set(keep);
	const autoKept = new Set<string>();
	const wanted = new Set<string>();
	const tx = db.transaction(['cascades', 'quizzes']);
	// Cascades the base holds, and any this device opened or kept before its rows arrived.
	const ids = new Set([...(await tx.objectStore('cascades').getAllKeys()), ...Object.keys(opens), ...keep]);
	for (const id of ids) {
		const c = { id };
		if (userKept.has(c.id)) {
			wanted.add(c.id);
			continue;
		}
		// A budget drop is remembered: until it is opened again the cascade is
		// treated as outside the window, automatic keep or not (§ Downloads),
		// or the next pass would fetch back what this one dropped.
		if (dropped.includes(c.id)) continue;
		if (opens[c.id] && !optout.includes(c.id)) {
			const active = (await tx.objectStore('quizzes').index('cascade_id').getAll(c.id))
				.filter((q) => q.status === 'active')
				.reduce((n, q) => n + q.question_count, 0);
			if (active > AUTO_KEEP_OFFLINE_ROWS) {
				autoKept.add(c.id);
				wanted.add(c.id);
				continue;
			}
		}
		const opened = opens[c.id];
		if (opened && now.getTime() - Date.parse(opened) < DOWNLOAD_WINDOW_DAYS * DAY_MS) {
			wanted.add(c.id);
		}
	}
	return { wanted, autoKept, userKept };
}

/** `question_rows_for`: the wanted cascades plus every cascade whose base holds question rows. */
export async function questionRowsFor(db: UserDb, now = new Date()): Promise<string[]> {
	const { wanted } = await policy(db, now);
	const ids = new Set(wanted);
	const tx = db.transaction(['cascades', 'quiz_questions']);
	for (const id of await tx.objectStore('cascades').getAllKeys()) {
		if (!ids.has(id) && (await tx.objectStore('quiz_questions').index('cascade_id').count(id)) > 0) ids.add(id);
	}
	return [...ids].sort();
}
