// Quiz options (PLAN.md § Quiz options): segment size is 0 (off) or 5 to
// MAX_QUIZ_QUESTIONS; forms clamp what they prefill to the cap in `meta`.
export interface QuizOptions {
	segment_size: number;
	progression: 'ladder' | 'drill';
	require_alphabetical: boolean;
}

export const MIN_SEGMENT = 5;

export function segmentSizeError(size: number, cap: number): string | null {
	if (size === 0) return null;
	if (!Number.isInteger(size) || size < MIN_SEGMENT || size > cap)
		return `Use 0 (off) or ${MIN_SEGMENT}–${cap.toLocaleString()}.`;
	return null;
}

/** The cap bounds new values only: a larger stored size is clamped when prefilled. */
export function clampPrefill(size: number, cap: number): number {
	return size > cap ? cap : size;
}

/** An attempt creates at most ceil(question_count / S) chain quizzes. */
export function chainBound(questionCount: number, size: number): number {
	return size > 0 && size < questionCount ? Math.ceil(questionCount / size) : 0;
}

export const DEFAULT_OPTIONS: QuizOptions = { segment_size: 0, progression: 'ladder', require_alphabetical: false };
