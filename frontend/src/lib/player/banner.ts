// The banners after moving on from the last card of an attempt or of a run
// (PLAN.md § Finishing a quiz). Everything is computed from what this device
// did; a finish that comes out differently on the server gets its notice from
// the rebase.
import type { Progression } from '$lib/cascade/rules';

export interface FinishFacts {
	level: number;
	correct: number;
	count: number;
	misses: number;
	threshold: number;
	progression: Progression;
	outcome: string;
	/** Chain quizzes: cleared with no misses returns to the parent's run. */
	segmentChain: boolean;
	completion?: { levels: number; attempts: number };
	/** The level and run the user returns to, for a cleared chain quiz. */
	back?: { level: number; run: number; runs: number };
}

/** Whole percent, rounded down, so a score just short of the threshold never reads as it. */
export function percent(correct: number, count: number): number {
	return count === 0 ? 0 : Math.floor((correct * 100) / count);
}

export function finishBanner(f: FinishFacts): string {
	const p = percent(f.correct, f.count);
	const n = f.level;
	switch (f.outcome) {
		case 'completed':
			return `Level 1: 100%. Cascade complete after ${f.completion!.levels} levels and ${f.completion!.attempts} attempts.`;
		case 'reshuffled':
			return `Level ${n}: 0%. Reshuffled. Try again.`;
		case 'descended':
			if (n === 1) return `Level 1: ${p}%. Reshuffled, and its ${f.misses} missed questions are now Level 2.`;
			return `Level ${n}: ${p}%, and ${f.threshold}% is needed to clear. Level ${n} is reshuffled and waiting. Down to Level ${n + 1} with ${f.misses} missed questions.`;
		case 'replaced':
			return `Level ${n}: ${p}%. Replaced with its ${f.misses} missed questions.`;
		case 'cleared':
			if (f.misses > 0) return `Level ${n} cleared with ${p}%. Its ${f.misses} missed questions are now Level ${n}.`;
			if (f.segmentChain && f.back) return `Level ${n} done. Back to Level ${f.back.level}, run ${f.back.run} of ${f.back.runs}.`;
			return `Level ${n} cleared with 100%. Back to Level ${n - 1}.`;
		default:
			return '';
	}
}

export function runBanner(o: { outcome: string; run: number; runs: number; misses: number; level: number }): string {
	if (o.outcome === 'drilled') return `Run ${o.run} of ${o.runs} done, ${o.misses} missed. Down to Level ${o.level + 1} to drill them.`;
	return `Run ${o.run} of ${o.runs} done, nothing missed. On to run ${o.run + 1}.`;
}
