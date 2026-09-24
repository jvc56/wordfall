// The player's state machine (PLAN.md § Taking a quiz, § Finishing a quiz,
// § Segments, § On the device). It reads the local view and writes only
// through `applyLocally`: Show / Next saves the grade and moves the cursor,
// or, at the last card of a run or of the attempt, emits only
// `finish_segment` or `finish`; Previous moves back, never below
// `run_start`. A card whose key never arrived reads "this question needs a
// connection" and is moved past with nothing emitted; a card whose answer is
// gone still grades, and a typed card falls back to flashcard mode.
import { applyLocally, APPLY_STORES, quizState, type LocalResult, type NewOp } from '$lib/local/apply';
import type { UserDb } from '$lib/local/db';
import type { CascadeRow, PreferencesRow, QuestionRow, QuizRow } from '$lib/local/rows';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { nextBoundary, Rejected, runIndicator, type Grade } from '$lib/cascade/rules';
import { Distribution } from '$lib/tiles';
import { finishBanner, runBanner } from './banner';
import { allFound, emptyTyped, submitTyped, typedGrade, type Submit, type TypedState } from './typed';

export interface Card {
	pos: number;
	idx: number;
	/** The question key, or null: "this question needs a connection". */
	key: string | null;
	/** The answer, or null: "answer needs a connection". */
	answer: unknown | null;
	revealed: boolean;
	grade: Grade;
	/** Marked missed in advance, before the reveal. */
	toggled: boolean;
	saved: Grade | null;
	/** Typed mode on this card; null in flashcard mode or when falling back. */
	typed: TypedState | null;
	/** The last typed entry's note: already entered, one word at a time, out of order. */
	note: string | null;
}

export type Status = 'loading' | 'downloading' | 'needs_connection' | 'ready' | 'trashed' | 'gone';

export interface PlayerView {
	status: Status;
	cascade: CascadeRow | null;
	quizzes: QuizRow[];
	quiz: QuizRow | null;
	card: Card | null;
	banner: string | null;
	/** A line in place of an action that could not act. */
	message: string | null;
	/** The storage message: the action waits for space (§ the quota path). */
	storage: boolean;
	completion: { levels: number; attempts: number } | null;
	/** Typed mode on a cascade whose distribution is missing. */
	distributionMissing: boolean;
}

export interface PlayerOptions {
	prefs: () => Promise<PreferencesRow>;
	/** Runs the full fetch sequence for the cascade (first open). */
	ensure: (cascadeId: string) => Promise<void>;
	afterWrite?: () => void;
	/** Fetches the card page holding `idx` ahead of any download (online only). */
	fetchCard?: (cascadeId: string, idx: number) => Promise<void>;
	seed?: () => string;
	uuid?: () => string;
}

const STORAGE = 'Not enough room on this device to save this answer.';

function randomSeed(): string {
	const a = new BigUint64Array(1);
	crypto.getRandomValues(a);
	return a[0].toString();
}

const isQuota = (e: unknown) => e instanceof DOMException && e.name === 'QuotaExceededError';

export class Player {
	v: PlayerView = {
		status: 'loading',
		cascade: null,
		quizzes: [],
		quiz: null,
		card: null,
		banner: null,
		message: null,
		storage: false,
		completion: null,
		distributionMissing: false
	};
	private order: number[] = [];
	private rows = new Map<number, QuestionRow>();
	private dist: Distribution | null = null;
	private retry: (() => Promise<void>) | null = null;
	private listeners = new Set<(v: PlayerView) => void>();
	private readonly seed: () => string;
	private readonly uuid: () => string;

	constructor(
		private readonly db: UserDb,
		readonly cascadeId: string,
		private readonly opts: PlayerOptions
	) {
		this.seed = opts.seed ?? randomSeed;
		this.uuid = opts.uuid ?? (() => crypto.randomUUID());
	}

	subscribe(fn: (v: PlayerView) => void): () => void {
		this.listeners.add(fn);
		fn(this.v);
		return () => this.listeners.delete(fn);
	}

	private emit() {
		this.v = { ...this.v };
		for (const fn of this.listeners) fn(this.v);
	}

	private tx() {
		return this.db.transaction(APPLY_STORES) as unknown as RwTx;
	}

	// -------------------------------------------------------------------------
	// Loading
	// -------------------------------------------------------------------------

	/** Reads the deepest level and the card at its cursor; `keep` keeps an in-progress card. */
	async load(keep = false): Promise<void> {
		const tx = this.tx();
		const cascade = (await view.cascade(tx, this.cascadeId)) ?? null;
		if (!cascade) {
			this.v = { ...this.v, status: 'gone', cascade: null };
			return this.emit();
		}
		const quizzes = await view.quizzesOf(tx, this.cascadeId);
		this.v.cascade = cascade;
		this.v.quizzes = quizzes;
		if (cascade.trashed_at !== null) {
			this.v.status = 'trashed';
			return this.emit();
		}
		const quiz = quizzes.find((q) => q.status === 'active' && q.level === cascade.depth) ?? null;
		if (!quiz) {
			this.v.status = 'gone';
			return this.emit();
		}
		let rows = await view.questionsOf(tx, quiz.id);
		if (quiz.pending || rows.length !== quiz.question_count || rows.some((r) => r.position === null)) {
			this.v.status = 'downloading';
			this.v.quiz = quiz;
			this.emit();
			try {
				await this.opts.ensure(this.cascadeId);
			} catch {
				this.v.status = 'needs_connection';
				return this.emit();
			}
			const again = this.tx();
			const q2 = await view.quiz(again, quiz.id);
			rows = await view.questionsOf(again, quiz.id);
			if (!q2 || q2.pending || rows.length !== q2.question_count || rows.some((r) => r.position === null)) {
				this.v.status = 'needs_connection';
				return this.emit();
			}
		}
		const prev = this.v.quiz;
		const same = keep && prev && prev.id === quiz.id && prev.attempt === quiz.attempt && prev.shuffle_seed === quiz.shuffle_seed;
		this.v.quiz = quiz;
		this.rows = new Map(rows.map((r) => [r.question_idx, r]));
		this.order = [...rows].sort((a, b) => a.position! - b.position!).map((r) => r.question_idx);
		const d = await this.db.get('distributions', cascade.letter_distribution);
		this.dist = d ? new Distribution(d.name, d.tiles) : null;
		this.v.status = 'ready';
		const pos = same && this.v.card ? Math.max(this.v.card.pos, quiz.cursor) : quiz.cursor;
		if (same && this.v.card && this.v.card.pos === pos) {
			this.v.card = { ...this.v.card, saved: this.rows.get(this.v.card.idx)?.grade ?? null };
			return this.emit();
		}
		await this.loadCard(pos);
	}

	private async loadCard(pos: number) {
		const idx = this.order[pos];
		let key = (await this.db.get('questions', [this.cascadeId, idx]))?.key ?? null;
		let card = await this.db.get('cards', [this.cascadeId, idx]);
		if ((key === null || !card) && this.opts.fetchCard) {
			// While online, the player fetches the page it needs on demand.
			try {
				await this.opts.fetchCard(this.cascadeId, idx);
				key = (await this.db.get('questions', [this.cascadeId, idx]))?.key ?? null;
				card = await this.db.get('cards', [this.cascadeId, idx]);
			} catch {
				// Offline: the card shows what the device holds.
			}
		}
		const saved = this.rows.get(idx)?.grade ?? null;
		const prefs = await this.opts.prefs();
		const typedWanted = this.v.cascade!.quiz_type === 'anagram' && prefs.anagram_answer_mode === 'typed';
		this.v.distributionMissing = typedWanted && !this.dist;
		// A card with no answer, or no distribution to split typed text, falls back to flashcard mode.
		const typed = typedWanted && card && this.dist && key && saved === null ? emptyTyped() : null;
		this.v.card = {
			pos,
			idx,
			key,
			answer: card?.answer ?? null,
			revealed: saved !== null,
			grade: saved ?? 'correct',
			toggled: false,
			saved,
			typed,
			note: null
		};
		this.v.message = null;
		this.emit();
	}

	// -------------------------------------------------------------------------
	// Writes
	// -------------------------------------------------------------------------

	private async write(op: NewOp): Promise<LocalResult | null> {
		try {
			const { result } = await applyLocally(this.db, op);
			this.v.storage = false;
			this.opts.afterWrite?.();
			return result;
		} catch (e) {
			if (isQuota(e)) {
				// The action reached neither the overlay nor the outbox: stay on this card.
				this.v.storage = true;
				this.v.message = STORAGE;
				this.emit();
				return null;
			}
			if (e instanceof Rejected) {
				await this.load();
				return null;
			}
			throw e;
		}
	}

	/** "Keep studying": the completion screen closes and stays closed. */
	dismissCompletion() {
		this.v.completion = null;
		this.emit();
	}

	/** After the user frees space: writes the action that waited. */
	async retryStorage() {
		const r = this.retry;
		this.retry = null;
		if (r) await r();
	}

	// -------------------------------------------------------------------------
	// The three actions
	// -------------------------------------------------------------------------

	private runEnd(q: QuizRow): number {
		return nextBoundary(quizState(q)) ?? q.question_count;
	}

	async showNext(): Promise<void> {
		const card = this.v.card;
		const q = this.v.quiz;
		if (!card || !q || this.v.status !== 'ready') return;
		this.v.banner = null;
		if (card.key === null) {
			// Moved past with nothing emitted; the end of a run waits for its keys.
			if (card.pos + 1 < this.runEnd(q)) await this.loadCard(card.pos + 1);
			else this.needsConnection(q);
			return;
		}
		if (!card.revealed) {
			this.v.card = {
				...card,
				revealed: true,
				grade: card.typed && card.answer ? typedGrade(card.typed, this.words(card)) : card.toggled ? 'missed' : 'correct'
			};
			return this.emit();
		}
		const act = async () => this.saveAndAdvance(card, q);
		this.retry = act;
		await act();
	}

	private needsConnection(q: QuizRow) {
		const start = q.run_start;
		const end = this.runEnd(q);
		let missing = 0;
		for (let p = start; p < end; p++) if (!this.rows.get(this.order[p])?.grade) missing++;
		this.v.message = `Some questions in this run still need a connection (${Math.max(missing, 1)}).`;
		this.emit();
	}

	private async saveAndAdvance(card: Card, q: QuizRow) {
		if (card.saved !== card.grade) {
			const r = await this.write({
				type: 'grade',
				quiz_id: q.id,
				attempt: q.attempt,
				attempt_seed: q.shuffle_seed,
				question_idx: card.idx,
				grade: card.grade
			});
			if (!r) return;
			this.rows.set(card.idx, { ...this.rows.get(card.idx)!, grade: card.grade });
		}
		this.retry = null;
		const end = this.runEnd(q);
		const next = card.pos + 1;
		if (next < end) {
			const r = await this.write({ type: 'move_cursor', quiz_id: q.id, attempt: q.attempt, attempt_seed: q.shuffle_seed, position: next });
			if (!r) return;
			await this.load();
			return;
		}
		// The last card of a run or of the attempt: every question in it must be graded.
		const start = end === q.question_count ? 0 : q.run_start;
		for (let p = start; p < end; p++) {
			if (!this.rows.get(this.order[p])?.grade) return this.needsConnection(q);
		}
		const current = (await view.quiz(this.tx(), q.id))!;
		const facts = {
			level: q.level,
			correct: current.correct_count,
			count: q.question_count,
			misses: current.missed_count,
			threshold: this.v.cascade!.clear_threshold,
			progression: q.progression,
			segmentChain: q.segment_chain
		};
		if (end === q.question_count) {
			const r = await this.write({
				type: 'finish',
				quiz_id: q.id,
				attempt: q.attempt,
				attempt_seed: q.shuffle_seed,
				shuffle_seed: this.seed(),
				new_quiz_id: this.uuid()
			});
			if (!r) return;
			await this.load();
			let back: { level: number; run: number; runs: number } | undefined;
			if (q.segment_chain && this.v.quiz) {
				const ri = runIndicator(quizState(this.v.quiz));
				if (ri) back = { level: this.v.quiz.level, run: ri.run, runs: ri.total };
			}
			this.v.banner = finishBanner({ ...facts, outcome: r.outcome!, completion: r.completion, back });
			if (r.completion) this.v.completion = r.completion;
			return this.emit();
		}
		const ri = runIndicator(quizState(q))!;
		let runMisses = 0;
		for (let p = q.run_start; p < end; p++) if (this.rows.get(this.order[p])?.grade === 'missed') runMisses++;
		const r = await this.write({
			type: 'finish_segment',
			quiz_id: q.id,
			attempt: q.attempt,
			attempt_seed: q.shuffle_seed,
			segment_end: end,
			shuffle_seed: this.seed(),
			new_quiz_id: this.uuid()
		});
		if (!r) return;
		await this.load();
		this.v.banner = runBanner({ outcome: r.outcome!, run: ri.run, runs: ri.total, misses: runMisses, level: q.level });
		this.emit();
	}

	async toggle(): Promise<void> {
		const card = this.v.card;
		if (!card || card.key === null || this.v.status !== 'ready') return;
		if (!card.revealed) this.v.card = { ...card, toggled: !card.toggled };
		else this.v.card = { ...card, grade: card.grade === 'correct' ? 'missed' : 'correct' };
		this.emit();
	}

	async previous(): Promise<void> {
		const card = this.v.card;
		const q = this.v.quiz;
		if (!card || !q || this.v.status !== 'ready') return;
		const target = card.pos - 1;
		if (target < q.run_start) {
			// A blocked Previous always has a visible cause.
			if (card.pos > 0 && !runIndicator(quizState(q))) this.v.message = 'You have already finished this part of the attempt.';
			return this.emit();
		}
		// Going back discards this card's unsaved reveal and toggle.
		if (q.cursor !== target) {
			const r = await this.write({ type: 'move_cursor', quiz_id: q.id, attempt: q.attempt, attempt_seed: q.shuffle_seed, position: target });
			if (!r) return;
			await this.load();
		} else {
			await this.loadCard(target);
		}
	}

	// -------------------------------------------------------------------------
	// Typed mode
	// -------------------------------------------------------------------------

	words(card: Card): string[] {
		return Array.isArray(card.answer) ? (card.answer as { word: string }[]).map((a) => a.word) : [];
	}

	/** Enter in the typed input; returns what happened for the input to show. */
	async enter(text: string): Promise<Submit | null> {
		const card = this.v.card;
		if (!card?.typed || card.revealed || !this.dist || !card.key) return null;
		const words = this.words(card);
		const { state, result } = submitTyped(
			card.typed,
			text,
			words,
			this.dist.parseMagpie(card.key, true).length,
			this.dist,
			this.v.quiz!.require_alphabetical
		);
		if (result.kind === 'empty') {
			await this.showNext();
			return result;
		}
		const notes: Record<string, string> = {
			already: 'Already entered',
			one_word: 'Type one word at a time',
			out_of_order: 'Out of order'
		};
		this.v.card = { ...card, typed: state, note: notes[result.kind] ?? null };
		if (allFound(state, words)) {
			this.v.card = { ...this.v.card, revealed: true, grade: typedGrade(state, words) };
		}
		this.emit();
		return result;
	}

	/**
	 * A download landed: the current card takes the key or answer it lacked, or
	 * the answer refetched with the definitions or hooks a preference now asks
	 * for, keeping its state.
	 */
	async refreshCard() {
		const card = this.v.card;
		if (!card) return;
		const key = (await this.db.get('questions', [this.cascadeId, card.idx]))?.key ?? null;
		const answer = (await this.db.get('cards', [this.cascadeId, card.idx]))?.answer ?? null;
		if (card.key === null && key !== null) return this.loadCard(card.pos);
		if (JSON.stringify(answer) === JSON.stringify(card.answer)) return;
		this.v.card = { ...card, answer };
		this.emit();
	}

	/** Switching answer modes in the middle of a card resets its typed entries. */
	async modeChanged() {
		if (this.v.card) await this.loadCard(this.v.card.pos);
	}
}
