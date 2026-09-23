// The levels of a cascade for the ladder panel (PLAN.md § Frontend →
// CascadeLadder). Nothing bounds a cascade's depth, so above twelve levels the
// ones between the Source quiz and the deepest few collapse into one row with
// their count, behind Show more; the Cascades page's compact ladder keeps
// Level 1 and the deepest level with "+N more" between.
import { runIndicator, runIndicatorText } from '$lib/cascade/rules';
import { quizState } from '$lib/local/apply';
import type { AttemptRow, QuizRow } from '$lib/local/rows';
import { percent } from './banner';

export const COLLAPSE_ABOVE = 12;
export const DEEPEST_KEPT = 3;

export interface LevelRow {
	level: number;
	quiz: QuizRow;
	size: number;
	attempt: number;
	/** The last finished attempt's score, as a whole percent. */
	lastScore: number | null;
	run: string | null;
	current: boolean;
	waiting: boolean;
	downloading: boolean;
}

export type LadderItem = { kind: 'level'; row: LevelRow } | { kind: 'more'; count: number };

export function levelRows(quizzes: QuizRow[], depth: number, attempts: AttemptRow[]): LevelRow[] {
	const active = quizzes.filter((q) => q.status === 'active').sort((a, b) => a.level - b.level);
	return active.map((q) => {
		const last = attempts.filter((a) => a.quiz_id === q.id).sort((a, b) => b.attempt - a.attempt)[0];
		const ri = runIndicator(quizState(q));
		return {
			level: q.level,
			quiz: q,
			size: q.question_count,
			attempt: q.attempt,
			lastScore: last ? percent(last.correct_count, last.question_count) : null,
			run: ri ? runIndicatorText(ri) : null,
			current: q.level === depth,
			waiting: q.level < depth,
			downloading: !!q.pending
		};
	});
}

/** The panel: all levels up to twelve; above, Level 1, a count, and the deepest few. */
export function ladderItems(rows: LevelRow[], expanded: boolean): LadderItem[] {
	if (expanded || rows.length <= COLLAPSE_ABOVE) return rows.map((row) => ({ kind: 'level', row }));
	const tail = rows.slice(-DEEPEST_KEPT);
	return [
		{ kind: 'level', row: rows[0] },
		{ kind: 'more', count: rows.length - 1 - tail.length },
		...tail.map((row) => ({ kind: 'level' as const, row }))
	];
}

/** The Cascades page's compact ladder: Level 1 and the deepest level with +N more between. */
export function compactItems(rows: LevelRow[], expanded: boolean): LadderItem[] {
	if (expanded || rows.length <= COLLAPSE_ABOVE) return rows.map((row) => ({ kind: 'level', row }));
	return [
		{ kind: 'level', row: rows[0] },
		{ kind: 'more', count: rows.length - 2 },
		{ kind: 'level', row: rows[rows.length - 1] }
	];
}
