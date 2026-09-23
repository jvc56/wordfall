// The rows the per-user stores hold (PLAN.md § On the device → IndexedDB
// stores). Base, overlay and staging rows share one shape per table: the
// server's wire row, with sequences as numbers (they stay far below 2^53),
// `shuffle_seed` and `questions_hash` as unsigned decimal text as on the wire
// (PQ-012), and `seq`, the sequence of the response that wrote the row
// (§ The sync cycle → Pull, 3).
import type { Grade, Origin, Outcome, Progression } from '$lib/cascade/rules';

export type QuizType = 'anagram' | 'definition' | 'leave_value';
export type QuizStatus = 'active' | 'cleared';

export interface CascadeRow {
	id: string;
	name: string;
	quiz_type: QuizType;
	lexicon: string;
	letter_distribution: string;
	clear_threshold: number;
	segment_size: number;
	progression: Progression;
	require_alphabetical: boolean;
	options_changed_at: string;
	options_seq: number;
	options_device_id: string;
	question_count: number;
	depth: number;
	peak_depth: number;
	attempts_since_completion: number;
	created_at: string;
	last_activity_at: string;
	completed_at: string | null;
	trashed_at: string | null;
	updated_seq: number;
	seq: number;
	/** Overlay only: the outbox purged it. */
	deleted?: boolean;
}

export interface QuizRow {
	id: string;
	cascade_id: string;
	level: number;
	origin: Origin;
	origin_quiz_id: string | null;
	origin_attempt: number | null;
	origin_segment_end: number | null;
	status: QuizStatus;
	segment_chain: boolean;
	segment_size: number;
	progression: Progression;
	require_alphabetical: boolean;
	options_changed_at: string;
	options_seq: number;
	options_device_id: string;
	attempt: number;
	shuffle_seed: string;
	questions_hash: string;
	question_count: number;
	correct_count: number;
	missed_count: number;
	cursor: number;
	cursor_moved_at: string | null;
	cursor_device_id: string | null;
	run_start: number;
	created_at: string;
	created_seq: number;
	last_activity_at: string;
	cleared_at: string | null;
	updated_seq: number;
	seq: number;
	/** Question rows yet to be fetched (§ On the device); a restore with no local rows sets it in the overlay. */
	pending?: boolean;
	/** Overlay only: the outbox purged it. */
	deleted?: boolean;
}

export interface QuestionRow {
	quiz_id: string;
	question_idx: number;
	cascade_id: string;
	/** Computed on the device from the quiz's seed; null until materialised. */
	position: number | null;
	grade: Grade | null;
	graded_at: string | null;
	seq: number;
}

export interface AttemptRow {
	quiz_id: string;
	attempt: number;
	cascade_id: string;
	question_count: number;
	correct_count: number;
	missed_count: number;
	outcome: Outcome;
	shuffle_seed: string;
	finished_at: string;
	updated_seq: number;
	seq: number;
}

/** A `questions` store entry: the key the card shows. */
export interface QuestionKey {
	cascade_id: string;
	idx: number;
	key: string;
}

/** A `cards` store entry: the answer, and which extras it holds. */
export interface CardRow {
	cascade_id: string;
	idx: number;
	answer: unknown;
	definitions: boolean;
	hooks: boolean;
}

export interface Binding {
	action: 'show_next' | 'toggle_grade' | 'previous';
	kind: 'key' | 'mouse_button' | 'wheel';
	code: string;
	ctrl: boolean;
	shift: boolean;
	alt: boolean;
	meta: boolean;
}

export interface PreferencesRow {
	default_clear_threshold: number;
	leave_value_decimals: number;
	anagram_show_definitions: boolean;
	anagram_show_hooks: boolean;
	anagram_answer_mode: 'flashcard' | 'typed';
	default_segment_size: number;
	default_progression: Progression;
	default_require_alphabetical: boolean;
	changed_at: string;
	changed_by_device_id: string | null;
	bindings_changed_at: string;
	bindings_device_id: string | null;
	updated_seq: number;
	bindings: Binding[];
}

export interface DistributionRow {
	name: string;
	tiles: { letter: string; blank_letter: string; value: number; is_vowel: boolean }[];
}

/** An outbox entry: the operation as sent, and what the device needs to find it. */
export interface OutboxEntry {
	device_seq: number;
	/** The cascade it touches; absent for preferences and bindings. */
	cascade_id?: string;
	/** Set on a `move_cursor` only, so a newer one finds the pending one. */
	cursor_quiz?: string;
	op: WireOp;
}

/** An operation in its wire form (PLAN.md § API → POST /api/sync). */
export interface WireOp {
	id: string;
	device_seq: number;
	seen_seq: number;
	at: string;
	type: string;
	[field: string]: unknown;
}

// ---------------------------------------------------------------------------
// From the wire
// ---------------------------------------------------------------------------

type Wire = Record<string, unknown>;

const num = (v: unknown) => (typeof v === 'string' ? Number(v) : (v as number));

/** A pulled cascade row, stamped with the response's sequence. */
export function cascadeFromWire(w: Wire, seq: number): CascadeRow {
	return { ...(w as unknown as CascadeRow), options_seq: num(w.options_seq), updated_seq: num(w.updated_seq), seq };
}

export function quizFromWire(w: Wire, seq: number): QuizRow {
	return {
		...(w as unknown as QuizRow),
		options_seq: num(w.options_seq),
		created_seq: num(w.created_seq),
		updated_seq: num(w.updated_seq),
		seq
	};
}

export function attemptFromWire(w: Wire, cascadeId: string, seq: number): AttemptRow {
	return { ...(w as unknown as AttemptRow), cascade_id: cascadeId, updated_seq: num(w.updated_seq), seq };
}

export function preferencesFromWire(w: Wire): PreferencesRow {
	const bindings = ((w.bindings as Wire[]) ?? []).map((b) => ({
		action: b.action,
		kind: b.kind,
		code: b.code,
		ctrl: b.ctrl,
		shift: b.shift,
		alt: b.alt,
		meta: b.meta
	})) as Binding[];
	return { ...(w as unknown as PreferencesRow), updated_seq: num(w.updated_seq), bindings };
}
