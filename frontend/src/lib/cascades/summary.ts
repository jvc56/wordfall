// What the Cascades and Trash pages show, from the local view (PLAN.md
// § Cascades page, § Trash page, § On the device → Downloads). Everything
// here works offline.
import { APPLY_STORES } from '$lib/local/apply';
import type { UserDb } from '$lib/local/db';
import { getMeta } from '$lib/local/meta';
import type { AttemptRow, CascadeRow, QuizRow } from '$lib/local/rows';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { levelRows, type LevelRow } from '$lib/player/ladder';
import { percent } from '$lib/player/banner';
import { DownloadManager } from '$lib/sync/downloads';
import { policy } from '$lib/sync/policy';
import { AUTO_KEEP_OFFLINE_ROWS } from '$lib/sync/config';

/** MAX_CASCADES_PER_USER's documented default; /api/auth/me does not report it (PQ-015). */
export const CASCADE_LIMIT = 100;
/** The builder warns from here (§ Cascade limit). */
export const CASCADE_WARNING = 90;

export type OfflineState = 'available' | 'downloading' | 'answers_need_connection' | 'not_downloaded';

/** The Keep offline toggle's hint for a cascade above AUTO_KEEP_OFFLINE_ROWS (§ Downloads). */
export function keepHint(s: CascadeSummary, opened: boolean): string | null {
	const active = s.levels.reduce((n, r) => n + r.size, 0);
	if (s.keptByUser || active <= AUTO_KEEP_OFFLINE_ROWS) return null;
	return opened ? (s.keptAutomatically ? 'kept automatically for its size' : null) : 'kept automatically once opened';
}

export interface CascadeSummary {
	cascade: CascadeRow;
	options: string;
	levels: LevelRow[];
	completedAt: string | null;
	offline: OfflineState;
	/** 0–100 while downloading. */
	progress: number;
	keptByUser: boolean;
	keptAutomatically: boolean;
}

/** `segments of 40 · drill`, or nothing when every option is at its default. */
export function optionsSummary(o: { segment_size: number; progression: string; require_alphabetical: boolean }): string {
	const parts: string[] = [];
	if (o.segment_size > 0) parts.push(`segments of ${o.segment_size}`);
	if (o.progression === 'drill') parts.push('drill');
	if (o.require_alphabetical) parts.push('alphabetical');
	return parts.join(' · ');
}

/** `L1 · 250 (waiting, run 3 of 7) → L2 · 38 (waiting) → L3 · 9 (4/9 done)`. */
export function ladderText(rows: LevelRow[]): string {
	return rows
		.map((r) => {
			const done = r.quiz.correct_count + r.quiz.missed_count;
			let state = r.current ? `${done}/${r.size} done` : 'waiting';
			if (r.downloading) state = 'waiting for download';
			const run = r.run ? `, ${r.run.split(' · ')[0]}` : '';
			return `L${r.level} · ${r.size} (${state}${run})`;
		})
		.join(' → ');
}

const tx = (db: UserDb) => db.transaction(APPLY_STORES) as unknown as RwTx;

/** Every cascade this device knows, trashed ones included (they count toward the limit). */
export async function allCascades(db: UserDb): Promise<CascadeRow[]> {
	const t = tx(db);
	const ids = new Set([...(await t.objectStore('cascades').getAllKeys()), ...(await t.objectStore('overlay_cascades').getAllKeys())]);
	const out: CascadeRow[] = [];
	for (const id of ids) {
		const c = await view.cascade(t, id);
		if (c) out.push(c);
	}
	return out;
}

export async function offlineState(db: UserDb, c: CascadeRow, online: boolean): Promise<{ state: OfflineState; progress: number }> {
	const dm = new DownloadManager(db);
	if (await dm.availableOffline(c.id)) return { state: 'available', progress: 100 };
	const rows = await db.countFromIndex('quiz_questions', 'cascade_id', c.id);
	const pending = (await db.countFromIndex('outbox', 'cascade_id', c.id)) > 0;
	const pol = await policy(db);
	if (rows === 0 && !pending && !pol.wanted.has(c.id)) return { state: 'not_downloaded', progress: 0 };
	const keys = await db.countFromIndex('questions', 'cascade_id', c.id);
	const cards = await db.countFromIndex('cards', 'cascade_id', c.id);
	// With its answers gone and no connection, there is no progress to show.
	if (!online && cards < c.question_count) return { state: 'answers_need_connection', progress: 0 };
	return { state: 'downloading', progress: Math.floor(((keys + cards) * 50) / Math.max(1, c.question_count)) };
}

/** The Cascades page: live cascades, newest activity first. */
export async function cascadeSummaries(db: UserDb, online: boolean): Promise<CascadeSummary[]> {
	const keep = new Set(await getMeta(db, 'keep_offline'));
	const pol = await policy(db);
	const out: CascadeSummary[] = [];
	for (const c of await allCascades(db)) {
		if (c.trashed_at !== null) continue;
		const t = tx(db);
		const quizzes = await view.quizzesOf(t, c.id);
		const attempts: AttemptRow[] = [];
		for (const q of quizzes) attempts.push(...(await view.attemptsOf(t, q.id)));
		const { state, progress } = await offlineState(db, c, online);
		out.push({
			cascade: c,
			options: optionsSummary(c),
			levels: levelRows(quizzes, c.depth, attempts),
			completedAt: c.completed_at,
			offline: state,
			progress,
			keptByUser: keep.has(c.id),
			keptAutomatically: pol.autoKept.has(c.id)
		});
	}
	return out.sort((a, b) => b.cascade.last_activity_at.localeCompare(a.cascade.last_activity_at));
}

// ---------------------------------------------------------------------------
// The Trash
// ---------------------------------------------------------------------------

export interface TrashEntry {
	kind: 'quiz' | 'cascade';
	id: string;
	cascade: CascadeRow;
	quiz?: QuizRow;
	/** The final score, and whether it cleared or was replaced. */
	score: number | null;
	outcome: string | null;
	/** When it will be purged; null while the date is provisional ("purges 30 days after this syncs"). */
	purgesAt: string | null;
	provisional: boolean;
}

export interface TrashGroup {
	cascade: CascadeRow;
	trashed: boolean;
	entries: TrashEntry[];
	earliest: string | null;
	/** The cascade's rows are not on this device: a restore "downloads when online". */
	notHere: boolean;
}

const DAY = 86_400_000;
const plus = (iso: string, days: number) => new Date(Date.parse(iso) + days * DAY).toISOString();

export async function trashGroups(db: UserDb): Promise<TrashGroup[]> {
	const days = (await getMeta(db, 'server')).trash_retention_days;
	const groups: TrashGroup[] = [];
	for (const c of await allCascades(db)) {
		const t = tx(db);
		const base = await t.objectStore('cascades').get(c.id);
		const entries: TrashEntry[] = [];
		const trashed = c.trashed_at !== null;
		if (trashed) {
			// A cascade trashed offline has only the provisional `trashed_at` the overlay holds.
			const provisional = !base?.trashed_at;
			entries.push({
				kind: 'cascade',
				id: c.id,
				cascade: c,
				score: null,
				outcome: null,
				purgesAt: provisional ? null : plus(c.trashed_at!, days),
				provisional
			});
		}
		for (const q of await view.quizzesOf(t, c.id)) {
			if (q.status !== 'cleared') continue;
			const bq = await t.objectStore('quizzes').get(q.id);
			const provisional = !bq?.cleared_at;
			const last = (await view.attemptsOf(t, q.id)).at(-1);
			entries.push({
				kind: 'quiz',
				id: q.id,
				cascade: c,
				quiz: q,
				score: last ? percent(last.correct_count, last.question_count) : null,
				outcome: last?.outcome ?? null,
				purgesAt: provisional || trashed ? null : plus(q.cleared_at!, days),
				provisional: provisional && !trashed
			});
		}
		if (!entries.length) continue;
		const dates = entries.map((e) => e.purgesAt).filter((d): d is string => d !== null);
		groups.push({
			cascade: c,
			trashed,
			entries,
			earliest: dates.length ? dates.sort()[0] : null,
			notHere: (await db.countFromIndex('quiz_questions', 'cascade_id', c.id)) === 0
		});
	}
	return groups.sort((a, b) => (a.earliest ?? '9').localeCompare(b.earliest ?? '9'));
}

/** A trashed cascade's quizzes can be restored and exported, but Delete forever is on the cascade alone. */
export function canDeleteForever(e: TrashEntry): boolean {
	return e.kind === 'cascade' || e.cascade.trashed_at === null;
}

export const PAGE = 100;
