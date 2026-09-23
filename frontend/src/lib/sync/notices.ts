// The notices a rebase shows (PLAN.md § Conflicts, the paragraphs after the
// table). A notice appears only when the user's own work was dropped: grades,
// and the finishes and quizzes built from them. Its first sentence follows the
// rejected or replay-dropped operation and its reason; the second, "K answers
// from this device weren't kept.", appears only when K > 0. Operations that
// depend on a rejected `finish`, `finish_segment` or `restore_quiz` — they
// name a quiz it was to create or an attempt seed it made — take no sentence
// of their own and are counted in its notice.
import { resetSeed } from '$lib/cascade/order';
import type { OutboxEntry } from '$lib/local/rows';

export interface Notice {
	cascade_id: string;
	text: string;
}

/** One operation of the batch, acknowledged or dropped in the replay. */
export interface Settled {
	entry: OutboxEntry;
	/** The server's or the replay's verdict. */
	status: 'applied' | 'rejected';
	reason?: string;
	/** An applied finish whose outcome came out differently (§ Conflicts). */
	mismatch?: boolean;
}

export interface NoticeContext {
	/** The cascade is gone from the base, or its tombstone arrived in this pull. */
	cascadeGone: boolean;
	/** The quiz exists here: a `not_found` grade on it is a question the server's copy lacks. */
	quizExists: (quizId: string) => boolean;
	/** A `bad_segment` whose cause was that its end is not past the server's cursor. */
	passedRun: (entry: OutboxEntry) => boolean;
}

const RESULT_BEARING = new Set(['finish', 'finish_segment', 'restore_quiz']);

const TRASHED = 'This cascade was moved to the Trash on another device.';
const DELETED = 'This cascade was deleted, on another device or by the Trash’s retention period.';
const differently = (n: number) =>
	`Level ${n} came out differently on the server because of changes made on another device.`;

function levelSentence(op: OutboxEntry, reason: string, ctx: NoticeContext): string {
	const n = op.local?.level ?? 1;
	const type = op.op.type;
	if (type === 'restore_quiz') {
		switch (reason) {
			case 'not_cleared':
				return 'This quiz was restored on another device.';
			case 'not_found':
				return 'This quiz was deleted, on another device or by the Trash’s retention period.';
			default:
				return 'This quiz could not be restored on the server.';
		}
	}
	switch (reason) {
		case 'stale_attempt':
		case 'not_active':
			return `Level ${n} was finished on another device.`;
		case 'not_deepest':
			return `Another level was added below Level ${n} on another device. Finish it first.`;
		case 'trashed':
			return TRASHED;
		case 'not_found':
			return DELETED;
		case 'duplicate_segment':
			return `This run of Level ${n} was finished on another device.`;
		case 'bad_segment':
			return ctx.passedRun(op)
				? `This run of Level ${n} was finished on another device.`
				: `Level ${n}’s segment size or place was changed on another device.`;
		case 'ungraded':
			return `Level ${n} has answers the server did not get. Finish it again.`;
		default:
			// invalid and error
			return `Level ${n} could not be saved on the server.`;
	}
}

function gradeSentence(op: OutboxEntry, reason: string, ctx: NoticeContext): string | null {
	const n = op.local?.level ?? 1;
	switch (reason) {
		case 'stale_attempt':
		case 'not_active':
			return `Level ${n} was finished on another device.`;
		case 'stale':
			return `Level ${n} was also answered on another device.`;
		case 'error':
			return `Level ${n}’s answers could not be saved on the server.`;
		case 'trashed':
			return TRASHED;
		case 'not_found':
			if (ctx.cascadeGone) return DELETED;
			if (ctx.quizExists(op.op.quiz_id as string)) return differently(n);
			return DELETED;
		default:
			return null;
	}
}

/** "K answers from this device weren't kept." — singular for one (PQ-014). */
export function keptSentence(k: number): string {
	if (k <= 0) return '';
	return k === 1 ? ' 1 answer from this device wasn’t kept.' : ` ${k} answers from this device weren’t kept.`;
}

interface Owner {
	sentence: string;
	count: number;
	quizzes: Set<string>;
	seeds: Set<string>;
}

function products(o: Owner, entry: OutboxEntry) {
	const op = entry.op;
	const quiz = op.quiz_id as string;
	if (op.type === 'finish') {
		o.quizzes.add(op.new_quiz_id as string);
		o.seeds.add(`${quiz}:${resetSeed(BigInt(op.shuffle_seed as string))}`);
	} else if (op.type === 'finish_segment') {
		o.quizzes.add(op.new_quiz_id as string);
	} else if (op.type === 'restore_quiz') {
		o.seeds.add(`${quiz}:${op.shuffle_seed as string}`);
	}
}

function dependsOn(o: Owner, entry: OutboxEntry): boolean {
	const op = entry.op;
	const quiz = op.quiz_id as string | undefined;
	if (!quiz) return false;
	if (o.quizzes.has(quiz)) return true;
	return op.attempt_seed !== undefined && o.seeds.has(`${quiz}:${op.attempt_seed as string}`);
}

/** The notices for one cascade's settled operations, in `device_seq` order. */
export function noticesFor(cascadeId: string, settled: Settled[], ctx: NoticeContext): Notice[] {
	const owners: Owner[] = [];
	const grouped = new Map<string, Owner>();
	const out: Owner[] = [];
	const ordered = [...settled].sort((a, b) => a.entry.device_seq - b.entry.device_seq);
	// Pass 1: the finishes, runs and restores that make a notice of their own,
	// each owning the attempt it finished and whatever it was to create; a
	// rejected one that depends on an earlier one joins it.
	for (const s of ordered) {
		const op = s.entry.op;
		if (!RESULT_BEARING.has(op.type)) continue;
		if (s.status === 'applied') {
			if (!s.mismatch) continue;
			const o: Owner = { sentence: differently(s.entry.local?.level ?? 1), count: 0, quizzes: new Set(), seeds: new Set() };
			products(o, s.entry);
			owners.push(o);
			out.push(o);
			continue;
		}
		const owner = owners.find((o) => dependsOn(o, s.entry));
		if (owner) {
			products(owner, s.entry);
			continue;
		}
		const o: Owner = { sentence: levelSentence(s.entry, s.reason ?? 'error', ctx), count: 0, quizzes: new Set(), seeds: new Set() };
		// The grades of the attempt a rejected finish or run was finishing are its own.
		if (op.type !== 'restore_quiz') o.seeds.add(`${op.quiz_id as string}:${op.attempt_seed as string}`);
		products(o, s.entry);
		owners.push(o);
		out.push(o);
	}
	// Pass 2: the grades.
	for (const s of ordered) {
		const op = s.entry.op;
		if (s.status === 'applied' || op.type !== 'grade') continue; // cursor moves, options, trash, restore and purge never make a notice
		const reason = s.reason ?? 'error';
		const owner = owners.find((o) => dependsOn(o, s.entry));
		if (owner) {
			owner.count++;
			continue;
		}
		const sentence = gradeSentence(s.entry, reason, ctx);
		if (!sentence) continue;
		let g = grouped.get(sentence);
		if (!g) {
			g = { sentence, count: 0, quizzes: new Set(), seeds: new Set() };
			grouped.set(sentence, g);
			out.push(g);
		}
		g.count++;
	}
	return out.map((o) => ({ cascade_id: cascadeId, text: o.sentence + keptSentence(o.count) }));
}
