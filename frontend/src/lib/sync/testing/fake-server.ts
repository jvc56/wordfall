// A small in-memory stand-in for POST /api/sync and the questions and grades
// endpoints, for the lib/sync unit tests. It applies operations through the
// shared rules (lib/cascade/sim.ts), keeps per-row sequences the way the
// server does, and pulls exactly the rows changed since the cursor, in table
// order, paged by total rows with the ceiling fixed by the first page
// (PLAN.md § The sync cycle). The server itself is tested in backend/tests.
import { questionsHash, toI64, fromI64 } from '$lib/cascade/order';
import { Rejected, type Grade, type Progression } from '$lib/cascade/rules';
import { SimCascade, type Op, type SimQuiz } from '$lib/cascade/sim';
import type { QuestionGroup, Reply, SyncRequest, SyncResult, Tombstone, WireRow } from '../protocol';

interface QMeta {
	graded_at: string;
	device: string;
	seq: number;
}

interface QuizMeta {
	updated_seq: number;
	created_seq: number;
	snapshot: string;
	cleared_at: string | null;
	cursor_at: string | null;
	cursor_device: string | null;
	cursor_seq: number;
	options_at: string;
	options_seq: number;
	options_device: string;
	questions: Map<number, QMeta>;
}

interface AttemptMeta {
	quiz_id: string;
	cascade_id: string;
	attempt: number;
	question_count: number;
	correct_count: number;
	missed_count: number;
	outcome: string;
	shuffle_seed: string;
	finished_at: string;
	updated_seq: number;
}

interface CascadeEntry {
	id: string;
	name: string;
	sim: SimCascade;
	updated_seq: number;
	snapshot: string;
	trashed_at: string | null;
	completed_at: string | null;
	options_at: string;
	options_seq: number;
	options_device: string;
	quizzes: Map<string, QuizMeta>;
}

export interface NewCascade {
	id: string;
	source_id: string;
	count: number;
	threshold?: number;
	segment_size?: number;
	progression?: Progression;
	seed?: bigint;
}

const T0 = '2026-01-01T00:00:00.000Z';

export class FakeServer {
	seq = 0;
	floor = 0;
	pageRows = 50_000;
	cascades = new Map<string, CascadeEntry>();
	quizCascade = new Map<string, string>();
	attempts: AttemptMeta[] = [];
	tombstones: (Tombstone & { seqn: number })[] = [];
	records = new Map<string, SyncResult>();
	prefsSeq = 1;
	prefsAt = T0;
	prefsDevice = '';
	prefs: Record<string, unknown> = {};
	/** Every request, for assertions. */
	log: SyncRequest[] = [];
	/** Replies to force before normal handling, e.g. a 503. */
	forced: Reply[] = [];

	constructor() {
		this.seq = 1;
	}

	// -------------------------------------------------------------------------
	// Setup
	// -------------------------------------------------------------------------

	create(c: NewCascade): { cascade: WireRow; source_quiz: WireRow; sync_seq: string } {
		const seq = ++this.seq;
		const opts = { segment_size: c.segment_size ?? 0, progression: c.progression ?? 'ladder', require_alphabetical: false };
		const sim = new SimCascade(c.threshold ?? 80, opts, c.count, c.source_id, c.seed ?? 0x8000_0000_0000_1234n);
		const e: CascadeEntry = {
			id: c.id,
			name: 'test',
			sim,
			updated_seq: seq,
			snapshot: '',
			trashed_at: null,
			completed_at: null,
			options_at: T0,
			options_seq: seq,
			options_device: 'server',
			quizzes: new Map()
		};
		this.cascades.set(c.id, e);
		this.track(e, seq, T0, 'server');
		return { cascade: this.cascadeRow(e), source_quiz: this.quizRow(e, sim.quizzes.get(c.source_id)!), sync_seq: String(seq) };
	}

	// -------------------------------------------------------------------------
	// Rows
	// -------------------------------------------------------------------------

	private cascadeRow(e: CascadeEntry): WireRow {
		const s = e.sim.state;
		const count = [...e.sim.quizzes.values()].find((q) => q.state.origin === 'source')?.state.question_count ?? 0;
		return {
			id: e.id,
			name: e.name,
			quiz_type: 'anagram',
			lexicon: 'EN-FIX',
			letter_distribution: 'english',
			clear_threshold: s.clear_threshold,
			segment_size: s.opts.segment_size,
			progression: s.opts.progression,
			require_alphabetical: s.opts.require_alphabetical,
			options_changed_at: e.options_at,
			options_seq: String(e.options_seq),
			options_device_id: e.options_device,
			question_count: count,
			depth: s.depth,
			peak_depth: s.peak_depth,
			attempts_since_completion: s.attempts_since_completion,
			created_at: T0,
			last_activity_at: T0,
			completed_at: e.completed_at,
			trashed_at: e.trashed_at,
			updated_seq: String(e.updated_seq)
		};
	}

	private quizRow(e: CascadeEntry, q: SimQuiz): WireRow {
		const m = e.quizzes.get(q.id)!;
		let cor = 0;
		let mis = 0;
		for (const g of q.grades.values()) (g === 'correct' ? cor++ : mis++);
		return {
			id: q.id,
			cascade_id: e.id,
			level: q.state.level,
			origin: q.state.origin,
			origin_quiz_id: q.origin_quiz_id,
			origin_attempt: q.origin_attempt,
			origin_segment_end: q.origin_segment_end,
			status: q.state.active ? 'active' : 'cleared',
			segment_chain: q.state.segment_chain,
			segment_size: q.state.opts.segment_size,
			progression: q.state.opts.progression,
			require_alphabetical: q.state.opts.require_alphabetical,
			options_changed_at: m.options_at,
			options_seq: String(m.options_seq),
			options_device_id: m.options_device,
			attempt: q.state.attempt,
			shuffle_seed: fromI64(toI64(q.state.seed)).toString(),
			questions_hash: questionsHash(q.questions).toString(),
			question_count: q.state.question_count,
			correct_count: cor,
			missed_count: mis,
			cursor: q.state.cursor,
			cursor_moved_at: m.cursor_at,
			cursor_device_id: m.cursor_device,
			run_start: q.state.run_start,
			created_at: T0,
			created_seq: String(m.created_seq),
			last_activity_at: T0,
			cleared_at: q.state.active ? null : m.cleared_at,
			updated_seq: String(m.updated_seq)
		};
	}

	/** Stamps every row an operation changed with `seq`. */
	private track(e: CascadeEntry, seq: number, at: string, device: string) {
		const snap = JSON.stringify({ ...e.sim.state, t: e.trashed_at });
		if (snap !== e.snapshot) {
			e.snapshot = snap;
			e.updated_seq = seq;
		}
		for (const q of e.sim.quizzes.values()) {
			this.quizCascade.set(q.id, e.id);
			let m = e.quizzes.get(q.id);
			if (!m) {
				m = {
					updated_seq: seq,
					created_seq: seq,
					snapshot: '',
					cleared_at: null,
					cursor_at: null,
					cursor_device: null,
					cursor_seq: 0,
					options_at: at,
					options_seq: seq,
					options_device: device,
					questions: new Map()
				};
				e.quizzes.set(q.id, m);
			}
			const qsnap = JSON.stringify({ ...q.state, seed: q.state.seed.toString(), g: [...q.grades] });
			if (qsnap !== m.snapshot) {
				if (m.snapshot && !q.state.active && m.cleared_at === null) m.cleared_at = at;
				if (q.state.active) m.cleared_at = null;
				m.snapshot = qsnap;
				m.updated_seq = seq;
				// Studying a quiz stamps its cascade too (last_activity_at).
				e.updated_seq = seq;
			}
			// Question rows: a reset clears them all; a grade stamps its own.
			for (const idx of [...m.questions.keys()]) if (!q.grades.has(idx)) m.questions.delete(idx);
		}
	}

	// -------------------------------------------------------------------------
	// POST /api/sync
	// -------------------------------------------------------------------------

	sync(req: SyncRequest): Reply {
		this.log.push(req);
		const forced = this.forced.shift();
		if (forced) return forced;
		if (req.page_token) return { status: 200, body: this.page(JSON.parse(req.page_token)), retryAfter: null };
		if (req.cursor !== null && req.cursor > this.seq) {
			return { status: 200, body: { results: [], sync_seq: String(this.seq), resync_required: true }, retryAfter: null };
		}
		const seq = ++this.seq;
		const results: SyncResult[] = [];
		const ops = [...((req.ops ?? []) as Record<string, unknown>[])].sort(
			(a, b) => (a.device_seq as number) - (b.device_seq as number)
		);
		for (const op of ops) results.push(this.apply(op, req.device_id, seq));
		if (req.cursor !== null && req.cursor < this.floor) {
			return { status: 200, body: { results, sync_seq: String(seq), resync_required: true }, retryAfter: null };
		}
		const body = { ...this.page({ cur: req.cursor, ceil: seq, qrf: req.question_rows_for, after: 0 }), results };
		return { status: 200, body, retryAfter: null };
	}

	private apply(w: Record<string, unknown>, device: string, seq: number): SyncResult {
		const id = w.id as string;
		const recorded = this.records.get(id);
		if (recorded) return recorded;
		let result: SyncResult;
		try {
			result = { op_id: id, status: 'applied', ...this.applyOp(w, device, seq) };
		} catch (e) {
			if (!(e instanceof Rejected)) throw e;
			result = { op_id: id, status: 'rejected', reason: e.reason };
		}
		this.records.set(id, result);
		return result;
	}

	private entryOf(w: Record<string, unknown>): CascadeEntry {
		const cid = (w.cascade_id as string) ?? this.quizCascade.get(w.quiz_id as string);
		const e = cid ? this.cascades.get(cid) : undefined;
		if (!e) throw new Rejected('not_found');
		return e;
	}

	private applyOp(w: Record<string, unknown>, device: string, seq: number): Partial<SyncResult> {
		const type = w.type as string;
		if (type === 'set_preferences' || type === 'set_bindings') {
			const seen = w.seen_seq as number;
			if (this.prefsDevice !== device && this.prefsSeq > seen && !((w.at as string) > this.prefsAt)) {
				throw new Rejected('stale');
			}
			Object.assign(this.prefs, w);
			this.prefsSeq = seq;
			this.prefsAt = w.at as string;
			this.prefsDevice = device;
			return {};
		}
		const e = this.entryOf(w);
		const at = w.at as string;
		const seen = w.seen_seq as number;
		const op = { ...w, quiz: w.quiz_id } as Record<string, unknown>;
		for (const k of ['attempt_seed', 'shuffle_seed']) if (k in w) op[k] = BigInt(w[k] as string);
		// The conflict rules for a grade: stale unless the device had seen the
		// current grade, set it itself, or is later (§ Conflicts).
		if (type === 'grade') {
			const m = e.quizzes.get(w.quiz_id as string)?.questions.get(w.question_idx as number);
			const q = e.sim.quizzes.get(w.quiz_id as string);
			if (q && m && q.grades.has(w.question_idx as number) && m.device !== device && m.seq > seen && !(at > m.graded_at)) {
				if (q.state.active && !e.sim.state.trashed && q.state.attempt === w.attempt && q.state.seed === op.attempt_seed) {
					throw new Rejected('stale');
				}
			}
		}
		if (type === 'purge_cascade' && e.sim.state.trashed) {
			const r = e.sim.apply(op as unknown as Op);
			for (const qid of e.quizzes.keys()) this.tombstones.push({ entity: 'quiz', entity_id: qid, seq: String(seq), seqn: seq });
			this.tombstones.push({ entity: 'cascade', entity_id: e.id, seq: String(seq), seqn: seq });
			this.cascades.delete(e.id);
			return r as Partial<SyncResult>;
		}
		const before = new Set(e.sim.quizzes.keys());
		const r = e.sim.apply(op as unknown as Op);
		if (type === 'trash_cascade') e.trashed_at = at;
		if (type === 'restore_cascade' || (type === 'restore_quiz' && !e.sim.state.trashed)) e.trashed_at = null;
		if (type === 'set_cascade_options') {
			e.options_at = at;
			e.options_seq = seq;
			e.options_device = device;
		}
		this.track(e, seq, at, device);
		const m = e.quizzes.get(w.quiz_id as string);
		if (type === 'grade' && m) m.questions.set(w.question_idx as number, { graded_at: at, device, seq });
		if (type === 'move_cursor' && m) Object.assign(m, { cursor_at: at, cursor_device: device, cursor_seq: seq });
		if (type === 'set_quiz_options' && m) Object.assign(m, { options_at: at, options_seq: seq, options_device: device });
		if (type === 'purge_quiz') {
			e.quizzes.delete(w.quiz_id as string);
			this.tombstones.push({ entity: 'quiz', entity_id: w.quiz_id as string, seq: String(seq), seqn: seq });
		}
		if (type === 'finish') {
			const q = e.sim.quizzes.get(w.quiz_id as string)!;
			this.attempts.push({
				quiz_id: q.id,
				cascade_id: e.id,
				attempt: w.attempt as number,
				question_count: q.state.question_count,
				correct_count: 0,
				missed_count: 0,
				outcome: r.outcome!,
				shuffle_seed: w.attempt_seed as string,
				finished_at: at,
				updated_seq: seq
			});
			if (r.completion) e.completed_at = at;
		}
		void before;
		const out: Partial<SyncResult> = {};
		if (r.outcome) out.outcome = r.outcome;
		if (r.new_quiz_question_count !== undefined) out.new_quiz_question_count = r.new_quiz_question_count;
		if (r.new_quiz_questions_hash !== undefined) out.new_quiz_questions_hash = r.new_quiz_questions_hash.toString();
		return out;
	}

	// -------------------------------------------------------------------------
	// Pull
	// -------------------------------------------------------------------------

	private page(t: { cur: number | null; ceil: number; qrf: string[]; after: number }) {
		const inRange = (s: number) => (t.cur === null || s > t.cur) && s <= t.ceil;
		type Item = { table: string; row: WireRow; n: number };
		const items: Item[] = [];
		const cascades = [...this.cascades.values()].sort((a, b) => (a.id < b.id ? -1 : 1));
		for (const e of cascades) if (inRange(e.updated_seq)) items.push({ table: 'cascades', row: this.cascadeRow(e), n: 1 });
		const quizzes: [CascadeEntry, SimQuiz][] = [];
		for (const e of cascades) for (const q of e.sim.quizzes.values()) quizzes.push([e, q]);
		quizzes.sort((a, b) => (a[1].id < b[1].id ? -1 : 1));
		for (const [e, q] of quizzes) {
			if (inRange(e.quizzes.get(q.id)!.updated_seq)) items.push({ table: 'quizzes', row: this.quizRow(e, q), n: 1 });
		}
		for (const a of this.attempts) {
			if (this.cascades.has(a.cascade_id) && inRange(a.updated_seq)) {
				const { cascade_id, ...row } = a;
				void cascade_id;
				items.push({ table: 'quiz_attempts', row: { ...row, updated_seq: String(a.updated_seq) }, n: 1 });
			}
		}
		for (const [e, q] of quizzes) {
			if (!q.state.active || !t.qrf.includes(e.id)) continue;
			const m = e.quizzes.get(q.id)!;
			const rows = [...q.grades.entries()]
				.filter(([idx]) => m.questions.has(idx) && inRange(m.questions.get(idx)!.seq))
				.sort((a, b) => a[0] - b[0]);
			for (const [idx, g] of rows) {
				const qm = m.questions.get(idx)!;
				items.push({ table: 'q', row: { quiz_id: q.id, idx, grade: g, graded_at: qm.graded_at, seq: qm.seq }, n: 1 });
			}
		}
		if (inRange(this.prefsSeq)) items.push({ table: 'preferences', row: { updated_seq: String(this.prefsSeq) }, n: 1 });
		if (t.cur !== null) {
			for (const tb of this.tombstones) {
				if (inRange(tb.seqn)) items.push({ table: 'tombstones', row: { entity: tb.entity, entity_id: tb.entity_id, seq: tb.seq }, n: 1 });
			}
		}
		const slice = items.slice(t.after, t.after + this.pageRows);
		const changes = {
			cascades: [] as WireRow[],
			quizzes: [] as WireRow[],
			quiz_attempts: [] as WireRow[],
			quiz_questions: [] as QuestionGroup[],
			tombstones: [] as Tombstone[]
		} as Record<string, unknown> & { quiz_questions: QuestionGroup[] };
		for (const it of slice) {
			if (it.table === 'q') {
				const r = it.row as { quiz_id: string; idx: number; grade: Grade; graded_at: string; seq: number };
				let g = changes.quiz_questions.at(-1);
				if (!g || g.quiz_id !== r.quiz_id) {
					g = { quiz_id: r.quiz_id, question_idx: [], grade: [], graded_at: [], min_updated_seq: String(r.seq) };
					changes.quiz_questions.push(g);
				}
				g.question_idx.push(r.idx);
				g.grade.push(r.grade);
				g.graded_at.push(r.graded_at);
				if (r.seq < Number(g.min_updated_seq)) g.min_updated_seq = String(r.seq);
			} else if (it.table === 'preferences') {
				changes.preferences = {
					default_clear_threshold: 80,
					leave_value_decimals: 1,
					anagram_show_definitions: false,
					anagram_show_hooks: false,
					anagram_answer_mode: 'flashcard',
					default_segment_size: 0,
					default_progression: 'ladder',
					default_require_alphabetical: false,
					...Object.fromEntries(
						Object.entries(this.prefs).filter(([k]) => k.startsWith('default_') || k.startsWith('leave_') || k.startsWith('anagram_'))
					),
					changed_at: this.prefsAt,
					changed_by_device_id: this.prefsDevice || null,
					bindings_changed_at: T0,
					bindings_device_id: null,
					updated_seq: String(this.prefsSeq),
					bindings: []
				};
			} else {
				(changes[it.table] as WireRow[]).push(it.row);
			}
		}
		const next = t.after + this.pageRows < items.length ? { ...t, after: t.after + this.pageRows } : null;
		return { results: [], changes, sync_seq: String(t.ceil), ...(next ? { next_page_token: JSON.stringify(next) } : {}) };
	}

	// -------------------------------------------------------------------------
	// The questions and grades endpoints
	// -------------------------------------------------------------------------

	questions(quizId: string, from: number, limit: number): number[] | null {
		const e = this.cascades.get(this.quizCascade.get(quizId) ?? '');
		const q = e?.sim.quizzes.get(quizId);
		if (!q || q.state.origin === 'source') return null;
		return [...q.questions].sort((a, b) => a - b).slice(from, from + limit);
	}

	grades(quizId: string, from: number, limit: number) {
		const e = this.cascades.get(this.quizCascade.get(quizId) ?? '');
		const q = e?.sim.quizzes.get(quizId);
		if (!q) return null;
		const m = e!.quizzes.get(quizId)!;
		const rows = [...q.grades.entries()].filter(([i]) => i >= from && i < from + limit).sort((a, b) => a[0] - b[0]);
		return {
			attempt: q.state.attempt,
			shuffle_seed: q.state.seed.toString(),
			question_idx: rows.map((r) => r[0]),
			grade: rows.map((r) => r[1]),
			graded_at: rows.map((r) => m.questions.get(r[0])?.graded_at ?? T0)
		};
	}

	/** GET …/cards?keys=1: the question keys, synthetic here. */
	keys(cascadeId: string, from: number, limit: number): { from: number; keys: string[] } | null {
		const e = this.cascades.get(cascadeId);
		if (!e) return null;
		const count = [...e.sim.quizzes.values()].find((q) => q.state.origin === 'source')!.state.question_count;
		const keys: string[] = [];
		for (let i = from; i < Math.min(count, from + limit); i++) keys.push(`KEY${i}`);
		return { from, keys };
	}

	/** GET …/cards: full cards; definitions only when asked for. */
	cards(cascadeId: string, from: number, limit: number, hooks: boolean, definitions: boolean) {
		const k = this.keys(cascadeId, from, limit);
		if (!k) return null;
		return k.keys.map((key, i) => ({
			idx: from + i,
			key,
			answer: [{ word: `WORD${from + i}`, ...(hooks ? { front_hooks: 'S' } : {}), ...(definitions ? { definition: 'a word' } : {}) }]
		}));
	}

	/** Prunes every tombstone, raising the floor to the highest removed. */
	pruneTombstones() {
		for (const t of this.tombstones) this.floor = Math.max(this.floor, t.seqn);
		this.tombstones = [];
	}
}
