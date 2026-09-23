// An in-memory cascade applying operations through the rules, in the order the
// server checks them: the TypeScript twin of backend/src/cascade/sim.rs, run
// by the shared vectors and the property test.
import { questionsHash, shuffle } from './order';
import {
	checkAttempt,
	checkDeepest,
	checkLive,
	checkMoveCursor,
	checkQuizOptionFields,
	checkSegmentEnd,
	checkSegmentPastCursor,
	checkSegmentSize,
	finish,
	finishSegment,
	outcomeOf,
	Rejected,
	restoreQuiz,
	type CascadeState,
	type Grade,
	type NewQuiz,
	type Opts,
	type Progression,
	type QuizState
} from './rules';

export interface SimQuiz {
	id: string;
	state: QuizState;
	origin_quiz_id: string | null;
	origin_attempt: number | null;
	origin_segment_end: number | null;
	questions: number[];
	grades: Map<number, Grade>;
}

export type Op =
	| { type: 'grade'; quiz: string; attempt: number; attempt_seed: bigint; question_idx: number; grade: Grade }
	| { type: 'move_cursor'; quiz: string; attempt: number; attempt_seed: bigint; position: number }
	| { type: 'finish'; quiz: string; attempt: number; attempt_seed: bigint; shuffle_seed: bigint; new_quiz_id: string }
	| {
			type: 'finish_segment';
			quiz: string;
			attempt: number;
			attempt_seed: bigint;
			segment_end: number;
			shuffle_seed: bigint;
			new_quiz_id: string;
	  }
	| { type: 'restore_quiz'; quiz: string; shuffle_seed: bigint }
	| { type: 'trash_cascade' }
	| { type: 'restore_cascade' }
	| { type: 'purge_quiz'; quiz: string }
	| { type: 'purge_cascade' }
	| { type: 'set_cascade_options'; segment_size?: number; progression?: Progression; require_alphabetical?: boolean }
	| {
			type: 'set_quiz_options';
			quiz: string;
			segment_size?: number;
			progression?: Progression;
			require_alphabetical?: boolean;
	  };

export interface Applied {
	outcome?: string;
	new_quiz_question_count?: number;
	new_quiz_questions_hash?: bigint;
	completion?: { levels: number; attempts: number };
}

export function positionsOf(q: SimQuiz): number[] {
	return shuffle(q.questions, q.state.seed);
}

function correct(q: SimQuiz) {
	let n = 0;
	for (const g of q.grades.values()) if (g === 'correct') n++;
	return n;
}

export class SimCascade {
	state: CascadeState;
	completions = 0;
	purged = false;
	quizzes = new Map<string, SimQuiz>();
	purgedQuizzes = new Set<string>();
	maxQuizQuestions = 300_000;

	constructor(threshold: number, opts: Opts, count: number, sourceId: string, sourceSeed: bigint) {
		this.state = {
			clear_threshold: threshold,
			opts: { ...opts },
			depth: 1,
			peak_depth: 1,
			attempts_since_completion: 0,
			trashed: false
		};
		this.quizzes.set(sourceId, {
			id: sourceId,
			state: {
				level: 1,
				origin: 'source',
				segment_chain: false,
				opts: { ...opts },
				attempt: 1,
				seed: sourceSeed,
				question_count: count,
				cursor: 0,
				run_start: 0,
				active: true
			},
			origin_quiz_id: null,
			origin_attempt: null,
			origin_segment_end: null,
			questions: Array.from({ length: count }, (_, i) => i),
			grades: new Map()
		});
	}

	deepest(): SimQuiz | undefined {
		for (const q of this.quizzes.values()) if (q.state.active && q.state.level === this.state.depth) return q;
		return undefined;
	}

	private quiz(id: string): SimQuiz {
		const q = this.purged ? undefined : this.quizzes.get(id);
		if (!q) throw new Rejected('not_found');
		return q;
	}

	private live(id: string): SimQuiz {
		const q = this.quiz(id);
		checkLive(this.state, q.state);
		return q;
	}

	private idFree(id: string) {
		if (this.quizzes.has(id) || this.purgedQuizzes.has(id)) throw new Rejected('invalid');
	}

	private insert(id: string, parent: string, n: NewQuiz): [number, bigint] {
		this.quizzes.set(id, {
			id,
			state: {
				level: n.level,
				origin: n.origin,
				segment_chain: n.segment_chain,
				opts: n.opts,
				attempt: 1,
				seed: n.seed,
				question_count: n.questions.length,
				cursor: 0,
				run_start: 0,
				active: true
			},
			origin_quiz_id: parent,
			origin_attempt: n.origin_attempt,
			origin_segment_end: n.origin_segment_end,
			questions: n.questions,
			grades: new Map()
		});
		return [n.questions.length, questionsHash(n.questions)];
	}

	private reset(q: SimQuiz, seed: bigint) {
		q.state.attempt += 1;
		q.state.seed = seed;
		q.state.cursor = 0;
		q.state.run_start = 0;
		q.grades = new Map();
	}

	/** Applies an operation, or throws `Rejected` with the reason. */
	apply(op: Op): Applied {
		switch (op.type) {
			case 'grade': {
				const q = this.live(op.quiz);
				checkAttempt(q.state, op.attempt, op.attempt_seed);
				if (!q.questions.includes(op.question_idx)) throw new Rejected('not_found');
				q.grades.set(op.question_idx, op.grade);
				return {};
			}
			case 'move_cursor': {
				const q = this.live(op.quiz);
				checkAttempt(q.state, op.attempt, op.attempt_seed);
				checkMoveCursor(q.state, op.position);
				q.state.cursor = op.position;
				return {};
			}
			case 'finish': {
				const q = this.live(op.quiz);
				checkDeepest(this.state, q.state);
				checkAttempt(q.state, op.attempt, op.attempt_seed);
				if (q.grades.size !== q.state.question_count) throw new Rejected('ungraded');
				this.idFree(op.new_quiz_id);
				const misses = q.questions.filter((i) => q.grades.get(i) === 'missed');
				const r = finish(this.state, q.state, correct(q), misses, op.shuffle_seed);
				const out: Applied = { outcome: outcomeOf(r) };
				if (r.kind === 'finished') {
					q.state.active = false;
					if (r.replacement) {
						const [c, h] = this.insert(op.new_quiz_id, q.id, r.replacement);
						out.new_quiz_question_count = c;
						out.new_quiz_questions_hash = h;
					}
				} else if (r.kind === 'descended') {
					this.reset(q, r.reset_seed);
					const [c, h] = this.insert(op.new_quiz_id, q.id, r.new_level);
					out.new_quiz_question_count = c;
					out.new_quiz_questions_hash = h;
				} else {
					if (r.kind === 'completed') {
						this.completions++;
						out.completion = { levels: r.levels, attempts: r.attempts };
					}
					this.reset(q, r.reset_seed);
					out.new_quiz_question_count = q.state.question_count;
					out.new_quiz_questions_hash = questionsHash(q.questions);
				}
				return out;
			}
			case 'finish_segment': {
				const q = this.live(op.quiz);
				checkDeepest(this.state, q.state);
				checkAttempt(q.state, op.attempt, op.attempt_seed);
				checkSegmentEnd(q.state, op.segment_end);
				for (const o of this.quizzes.values()) {
					if (
						o.state.origin === 'segment' &&
						o.origin_quiz_id === q.id &&
						o.origin_attempt === q.state.attempt &&
						o.origin_segment_end === op.segment_end
					)
						throw new Rejected('duplicate_segment');
				}
				checkSegmentPastCursor(q.state, op.segment_end);
				const pos = positionsOf(q);
				for (let p = 0; p < op.segment_end; p++) if (!q.grades.has(pos[p])) throw new Rejected('ungraded');
				this.idFree(op.new_quiz_id);
				const runMisses = pos
					.slice(q.state.run_start, op.segment_end)
					.filter((i) => q.grades.get(i) === 'missed')
					.sort((a, b) => a - b);
				const r = finishSegment(this.state, q.state, runMisses, op.segment_end, op.shuffle_seed);
				const out: Applied = {};
				if (r.kind === 'drilled') {
					const [c, h] = this.insert(op.new_quiz_id, q.id, r.new_level);
					out.outcome = 'drilled';
					out.new_quiz_question_count = c;
					out.new_quiz_questions_hash = h;
				} else out.outcome = 'continued';
				q.state.cursor = op.segment_end;
				q.state.run_start = op.segment_end;
				return out;
			}
			case 'restore_quiz': {
				const q = this.quiz(op.quiz);
				if (q.state.active) throw new Rejected('not_cleared');
				const r = restoreQuiz(this.state, q.state, op.shuffle_seed);
				this.reset(q, r.seed);
				q.state.attempt = r.attempt;
				q.state.level = r.level;
				q.state.opts = r.opts;
				q.state.active = true;
				return { new_quiz_question_count: q.state.question_count, new_quiz_questions_hash: questionsHash(q.questions) };
			}
			case 'trash_cascade':
				if (this.purged) throw new Rejected('not_found');
				if (this.state.trashed) throw new Rejected('trashed');
				this.state.trashed = true;
				return {};
			case 'restore_cascade':
				if (this.purged) throw new Rejected('not_found');
				if (!this.state.trashed) throw new Rejected('not_trashed');
				this.state.trashed = false;
				return {};
			case 'purge_quiz': {
				const q = this.quiz(op.quiz);
				if (this.state.trashed) throw new Rejected('trashed');
				if (q.state.active) throw new Rejected('not_cleared');
				this.quizzes.delete(q.id);
				this.purgedQuizzes.add(q.id);
				return {};
			}
			case 'purge_cascade':
				if (this.purged) throw new Rejected('not_found');
				if (!this.state.trashed) throw new Rejected('not_trashed');
				this.purged = true;
				for (const id of this.quizzes.keys()) this.purgedQuizzes.add(id);
				this.quizzes.clear();
				return {};
			case 'set_cascade_options': {
				if (this.purged) throw new Rejected('not_found');
				if (this.state.trashed) throw new Rejected('trashed');
				const size = op.segment_size === undefined ? undefined : checkSegmentSize(op.segment_size, this.maxQuizQuestions);
				if (size !== undefined) this.state.opts.segment_size = size;
				if (op.progression !== undefined) this.state.opts.progression = op.progression;
				if (op.require_alphabetical !== undefined) this.state.opts.require_alphabetical = op.require_alphabetical;
				return {};
			}
			case 'set_quiz_options': {
				const q = this.live(op.quiz);
				const size = op.segment_size === undefined ? undefined : checkSegmentSize(op.segment_size, this.maxQuizQuestions);
				checkQuizOptionFields(q.state, op.progression !== undefined, op.segment_size !== undefined);
				if (size !== undefined) q.state.opts.segment_size = size;
				if (op.progression !== undefined) q.state.opts.progression = op.progression;
				if (op.require_alphabetical !== undefined) q.state.opts.require_alphabetical = op.require_alphabetical;
				return {};
			}
		}
	}
}
