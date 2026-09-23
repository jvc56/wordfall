// The cascade rules as pure functions (PLAN.md § Cascade Rules, § Rule
// implementation, § Operations), the TypeScript twin of
// backend/src/cascade/rules.rs. Both pass contract-fixtures/cascade/.
//
// Each function returns what must change; the completion counters are the one
// thing they change themselves, on the cascade state they are given.

import { resetSeed } from './order';

export type Progression = 'ladder' | 'drill';
export type Origin = 'source' | 'clear_replacement' | 'drill_replacement' | 'descent' | 'segment';
export type Grade = 'correct' | 'missed';
export type Outcome = 'cleared' | 'replaced' | 'descended' | 'completed' | 'reshuffled';
export type Reason =
	| 'not_found'
	| 'trashed'
	| 'not_active'
	| 'not_deepest'
	| 'stale_attempt'
	| 'ungraded'
	| 'bad_segment'
	| 'duplicate_segment'
	| 'stale'
	| 'not_cleared'
	| 'not_trashed'
	| 'bad_cursor'
	| 'invalid'
	| 'error';

export class Rejected extends Error {
	constructor(readonly reason: Reason) {
		super(reason);
	}
}

export interface Opts {
	segment_size: number;
	progression: Progression;
	require_alphabetical: boolean;
}

export interface CascadeState {
	clear_threshold: number;
	opts: Opts;
	depth: number;
	peak_depth: number;
	attempts_since_completion: number;
	trashed: boolean;
}

export interface QuizState {
	level: number;
	origin: Origin;
	segment_chain: boolean;
	opts: Opts;
	attempt: number;
	seed: bigint;
	question_count: number;
	cursor: number;
	run_start: number;
	active: boolean;
}

export interface NewQuiz {
	level: number;
	origin: Origin;
	/** Ascending. */
	questions: number[];
	seed: bigint;
	segment_chain: boolean;
	opts: Opts;
	origin_attempt: number;
	origin_segment_end: number | null;
}

export type FinishResult =
	| { kind: 'finished'; cleared: boolean; replacement: NewQuiz | null }
	| { kind: 'descended'; reset_seed: bigint; new_level: NewQuiz }
	| { kind: 'completed'; reset_seed: bigint; levels: number; attempts: number }
	| { kind: 'reshuffled'; reset_seed: bigint };

export function outcomeOf(r: FinishResult): Outcome {
	switch (r.kind) {
		case 'finished':
			return r.cleared ? 'cleared' : 'replaced';
		default:
			return r.kind;
	}
}

export type SegmentResult = { kind: 'drilled'; new_level: NewQuiz } | { kind: 'continued' };

export interface Restored {
	level: number;
	attempt: number;
	seed: bigint;
	opts: Opts;
	cascade_restored: boolean;
}

/** A created quiz's options: the cascade's, except a chain: Drill, no segments. */
export function newQuizOpts(c: CascadeState, chain: boolean): Opts {
	return chain
		? { segment_size: 0, progression: 'drill', require_alphabetical: c.opts.require_alphabetical }
		: { ...c.opts };
}

export function effectiveSegmentSize(q: QuizState): number {
	const s = q.opts.segment_size;
	return q.segment_chain || s === 0 || s >= q.question_count ? 0 : s;
}

/** The smallest multiple of the segment size above the cursor and below the
 * count; null when off, too large, or a chain — nothing divides by the size. */
export function nextBoundary(q: QuizState): number | null {
	const s = effectiveSegmentSize(q);
	if (s === 0) return null;
	const b = (Math.floor(q.cursor / s) + 1) * s;
	return b < q.question_count ? b : null;
}

export interface RunIndicator {
	run: number;
	total: number;
	position: number;
	length: number;
}

export function runIndicator(q: QuizState): RunIndicator | null {
	const s = effectiveSegmentSize(q);
	if (s === 0) return null;
	const end = nextBoundary(q) ?? q.question_count;
	return {
		run: Math.floor(q.run_start / s) + 1,
		total: Math.floor((q.question_count - 1) / s) + 1,
		position: q.cursor - q.run_start + 1,
		length: end - q.run_start
	};
}

export function runIndicatorText(r: RunIndicator): string {
	return `run ${r.run} of ${r.total} · ${r.position} of ${r.length}`;
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

export function checkLive(c: CascadeState, q: QuizState) {
	if (c.trashed) throw new Rejected('trashed');
	if (!q.active) throw new Rejected('not_active');
}

export function checkAttempt(q: QuizState, attempt: number, attemptSeed: bigint) {
	if (attempt !== q.attempt || attemptSeed !== q.seed) throw new Rejected('stale_attempt');
}

export function checkDeepest(c: CascadeState, q: QuizState) {
	if (q.level !== c.depth) throw new Rejected('not_deepest');
}

export function checkMoveCursor(q: QuizState, position: number) {
	const limit = nextBoundary(q) ?? q.question_count;
	if (position < q.run_start || position >= limit) throw new Rejected('bad_cursor');
}

export function checkSegmentEnd(q: QuizState, end: number) {
	const s = q.opts.segment_size;
	if (q.segment_chain || s === 0 || end % s !== 0 || end === 0 || end >= q.question_count)
		throw new Rejected('bad_segment');
}

export function checkSegmentPastCursor(q: QuizState, end: number) {
	if (end <= q.cursor) throw new Rejected('bad_segment');
}

export function checkSegmentSize(size: number, cap: number): number {
	if (Number.isInteger(size) && (size === 0 || (size >= 5 && size <= cap))) return size;
	throw new Rejected('invalid');
}

export function checkQuizOptionFields(q: QuizState, progression: boolean, segmentSize: boolean) {
	if (q.segment_chain && (progression || segmentSize)) throw new Rejected('invalid');
	if (q.origin === 'source' && progression) throw new Rejected('invalid');
}

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

function pushDepth(c: CascadeState) {
	c.depth += 1;
	c.peak_depth = Math.max(c.peak_depth, c.depth);
}

/** Finishing an attempt. `misses` ascending. */
export function finish(c: CascadeState, q: QuizState, correct: number, misses: number[], seed: bigint): FinishResult {
	const count = q.question_count;
	// Compared exactly: correct × 100 ≥ threshold × question_count.
	const passed = correct * 100 >= c.clear_threshold * count;
	c.attempts_since_completion += 1;
	const reset = resetSeed(seed);
	const make = (level: number, origin: Origin, chain: boolean): NewQuiz => ({
		level,
		origin,
		questions: misses,
		seed,
		segment_chain: chain,
		opts: newQuizOpts(c, chain),
		origin_attempt: q.attempt,
		origin_segment_end: null
	});
	if (q.level === 1) {
		if (misses.length === 0) {
			const levels = c.peak_depth;
			const attempts = c.attempts_since_completion;
			c.peak_depth = 1;
			c.attempts_since_completion = 0;
			return { kind: 'completed', reset_seed: reset, levels, attempts };
		}
		if (correct === 0) return { kind: 'reshuffled', reset_seed: reset };
		const n = make(2, 'descent', false);
		pushDepth(c);
		return { kind: 'descended', reset_seed: reset, new_level: n };
	}
	if (correct === 0) return { kind: 'reshuffled', reset_seed: reset };
	if (q.opts.progression === 'ladder' && !passed) {
		const n = make(q.level + 1, 'descent', false);
		pushDepth(c);
		return { kind: 'descended', reset_seed: reset, new_level: n };
	}
	if (misses.length === 0) {
		c.depth -= 1;
		return { kind: 'finished', cleared: passed, replacement: null };
	}
	return {
		kind: 'finished',
		cleared: passed,
		replacement: make(q.level, passed ? 'clear_replacement' : 'drill_replacement', q.segment_chain)
	};
}

/** The last question of a run that is not the last run. */
export function finishSegment(c: CascadeState, q: QuizState, runMisses: number[], end: number, seed: bigint): SegmentResult {
	if (runMisses.length === 0) return { kind: 'continued' };
	const n: NewQuiz = {
		level: q.level + 1,
		origin: 'segment',
		questions: runMisses,
		seed,
		segment_chain: true,
		opts: newQuizOpts(c, true),
		origin_attempt: q.attempt,
		origin_segment_end: end
	};
	pushDepth(c);
	return { kind: 'drilled', new_level: n };
}

/** Restoring a cleared quiz as the new deepest level. */
export function restoreQuiz(c: CascadeState, q: QuizState, seed: bigint): Restored {
	const cascadeRestored = c.trashed;
	c.trashed = false;
	pushDepth(c);
	const opts = q.segment_chain ? { ...q.opts, require_alphabetical: c.opts.require_alphabetical } : { ...c.opts };
	return { level: c.depth, attempt: q.attempt + 1, seed, opts, cascade_restored: cascadeRestored };
}
