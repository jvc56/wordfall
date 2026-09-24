// The device's half of PLAN.md § Scale tests: a device (IndexedDB through
// fake-indexeddb, the real sync engine and download manager) against the
// in-memory server, with every request counted and weighed and every
// IndexedDB write counted. `make test-scale` runs the *.scale.ts files that
// use it (vitest.scale.config.ts), each in its own process.
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { applyLocally, APPLY_STORES, type NewOp } from '$lib/local/apply';
import { openUserDb, type UserDb, type StoreName } from '$lib/local/db';
import { initMeta, recordOpen } from '$lib/local/meta';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { DownloadManager, type DownloadApi } from '../downloads';
import { SyncEngine } from '../engine';
import type { SyncRequest, SyncResponse } from '../protocol';
import { FakeServer } from './fake-server';

export const QUESTIONS = 300_000;
export const DAY = 86_400_000;

/** IndexedDB writes (put, add, delete, clear), counted across every database. */
export const writes = { n: 0 };
let counting = false;

function countWrites() {
	if (counting) return;
	counting = true;
	const proto = (globalThis as unknown as { IDBObjectStore: { prototype: Record<string, (...a: unknown[]) => unknown> } }).IDBObjectStore.prototype;
	for (const m of ['put', 'add', 'delete', 'clear']) {
		const f = proto[m];
		proto[m] = function (this: unknown, ...a: unknown[]) {
			writes.n++;
			return f.apply(this, a);
		};
	}
}

export interface Call {
	kind: string;
	cascade: string;
	bytes: number;
}

/** Milliseconds `fn` took, and its result. */
export async function timed<T>(fn: () => Promise<T>): Promise<[T, number]> {
	const t = performance.now();
	const r = await fn();
	return [r, performance.now() - t];
}

export function report(what: string, ms: number, budgetMs: number | null, detail: string) {
	const b = budgetMs === null ? 'recorded' : `of ${(budgetMs / 1000).toFixed(0)} s`;
	console.log(`scale: ${what}: ${(ms / 1000).toFixed(1)} s ${b} (${detail})`);
}

/** A cascade id and its Source quiz id, the nth of a run. */
export function ids(n: number): { cascade: string; source: string } {
	const h = n.toString(16).padStart(12, '0');
	return { cascade: `c0000000-0000-4000-8000-${h}`, source: `a0000000-0000-4000-8000-${h}` };
}

export class ScaleDev {
	db!: UserDb;
	engine!: SyncEngine;
	dl!: DownloadManager;
	calls: Call[] = [];
	requests: SyncRequest[] = [];
	responses: SyncResponse[] = [];
	now = Date.now();
	/** Refuses a download call (e.g. answers, to stop a pass after its keys). */
	refuse: ((c: Call) => boolean) | null = null;
	openHere: string | null = null;

	static async make(server: FakeServer, user = '33333333-3333-4333-8333-333333333333', o: { rowBudget?: number } = {}) {
		countWrites();
		const d = new ScaleDev();
		globalThis.indexedDB = new IDBFactory();
		d.db = await openUserDb(user);
		await initMeta(d.db, user, 'scale');
		const clock = () => new Date(d.now);
		d.engine = new SyncEngine(
			d.db,
			{},
			{
				transport: async (r) => {
					const req = JSON.parse(JSON.stringify(r)) as SyncRequest;
					d.requests.push(req);
					const reply = server.sync(req);
					d.responses.push(reply.body as SyncResponse);
					return reply;
				},
				wait: async () => undefined,
				now: clock
			}
		);
		const weigh = <T>(kind: string, cascade: string, r: T): T => {
			const c = { kind, cascade, bytes: JSON.stringify(r).length };
			if (d.refuse?.(c)) throw new TypeError('offline');
			d.calls.push(c);
			return r;
		};
		const api: DownloadApi = {
			questions: async (cascade, quiz, from, limit) => weigh('questions', cascade, server.questions(quiz, from, limit)!),
			grades: async (cascade, quiz, from, limit) => weigh('grades', cascade, server.grades(quiz, from, limit)!),
			keys: async (cascade, from, limit) => weigh('keys', cascade, server.keys(cascade, from, limit)!),
			cards: async (cascade, from, limit, hooks, definitions) => weigh('cards', cascade, server.cards(cascade, from, limit, hooks, definitions)!),
			distribution: async (name) => weigh('distribution', name, { name, tiles: [] })
		};
		d.dl = new DownloadManager(d.db, {
			api,
			sync: () => d.engine.sync(),
			wait: async () => undefined,
			now: clock,
			openHere: () => d.openHere,
			openElsewhere: async () => null,
			...o
		});
		return d;
	}

	async open(...cascades: string[]) {
		const tx = this.db.transaction(['meta'], 'readwrite');
		for (const c of cascades) await recordOpen(tx as unknown as RwTx, c, new Date(this.now).toISOString());
		await tx.done;
	}

	/** A sync, then the download manager's pass, as the engine's hook runs it. */
	async sync() {
		await this.engine.sync();
		await this.dl.run();
	}

	/** Syncs until the outbox is empty; returns the requests it took. */
	async drain(): Promise<number> {
		const before = this.requests.length;
		while ((await this.db.count('outbox')) > 0) await this.engine.sync();
		return this.requests.length - before;
	}

	count(store: StoreName) {
		return this.db.count(store);
	}

	countFor(store: 'quiz_questions' | 'questions' | 'cards', cascadeId: string) {
		return this.db.countFromIndex(store, 'cascade_id', cascadeId);
	}

	apply(op: NewOp) {
		return applyLocally(this.db, op);
	}

	quiz(id: string) {
		return view.quiz(this.db.transaction(APPLY_STORES) as unknown as RwTx, id);
	}

	/** Grades every card of the quiz, locally, in position order; `missed(p)` by position. */
	async gradeAll(quizId: string, missed: (p: number) => boolean) {
		const q = (await this.quiz(quizId))!;
		const rows = await view.questionsOf(this.db.transaction(APPLY_STORES) as unknown as RwTx, quizId);
		rows.sort((a, b) => a.position! - b.position!);
		for (const r of rows) {
			await this.apply({
				type: 'grade',
				quiz_id: quizId,
				attempt: q.attempt,
				attempt_seed: q.shuffle_seed,
				question_idx: r.question_idx,
				grade: missed(r.position!) ? 'missed' : 'correct'
			});
		}
	}

	/** Bytes of what a store holds, as structured JSON. */
	async bytes(store: StoreName, pick?: (v: Record<string, unknown>) => unknown): Promise<number> {
		let n = 0;
		let cursor = await this.db.transaction(store).store.openCursor();
		while (cursor) {
			const v = cursor.value as Record<string, unknown>;
			n += JSON.stringify(pick ? pick(v) : v).length;
			cursor = await cursor.continue();
		}
		return n;
	}
}

/**
 * Plays a quiz on the server as another device would: grades from position
 * `from` for `n` cards (all of it by default), `missed(p)` by position, in
 * requests of `batch` operations.
 */
export class ServerPlayer {
	private seqn = 1;
	cursor: number | null = null;
	constructor(
		private server: FakeServer,
		private device = 'ffffffff-ffff-4fff-8fff-ffffffffffff'
	) {}

	private op(type: string, fields: Record<string, unknown>) {
		return { id: crypto.randomUUID(), device_seq: this.seqn++, seen_seq: this.cursor ?? 0, at: new Date().toISOString(), type, ...fields };
	}

	send(ops: Record<string, unknown>[]) {
		const r = this.server.sync({ device_id: this.device, app_version: 1, cursor: this.cursor, question_rows_for: [], ops } as unknown as SyncRequest);
		const body = r.body as SyncResponse;
		for (const x of body.results) if (x.status !== 'applied') throw new Error(`${x.status} ${x.reason}`);
		return body;
	}

	private orders = new Map<string, number[]>();

	/** The quiz's question order: the sim's positions, for this attempt's seed. */
	order(cascade: string, quiz: string): number[] {
		const key = `${quiz}:${this.server.quizState(cascade, quiz).seed}`;
		let o = this.orders.get(key);
		if (!o) {
			o = this.server.positions(cascade, quiz);
			this.orders.set(key, o);
		}
		return o;
	}

	grade(cascade: string, quiz: string, missed: (p: number) => boolean, from = 0, n?: number, batch = 50_000) {
		const q = this.server.quizState(cascade, quiz);
		const order = this.order(cascade, quiz).slice(from, n === undefined ? undefined : from + n);
		for (let i = 0; i < order.length; i += batch) {
			this.send(
				order.slice(i, i + batch).map((idx, j) =>
					this.op('grade', { quiz_id: quiz, attempt: q.attempt, attempt_seed: q.seed.toString(), question_idx: idx, grade: missed(from + i + j) ? 'missed' : 'correct' })
				)
			);
		}
	}

	finish(cascade: string, quiz: string, seed: bigint, newQuiz: string) {
		const q = this.server.quizState(cascade, quiz);
		return this.send([this.op('finish', { quiz_id: quiz, attempt: q.attempt, attempt_seed: q.seed.toString(), shuffle_seed: seed.toString(), new_quiz_id: newQuiz })]).results[0];
	}

	finishSegment(cascade: string, quiz: string, end: number, seed: bigint, newQuiz: string) {
		const q = this.server.quizState(cascade, quiz);
		return this.send([
			this.op('finish_segment', { quiz_id: quiz, attempt: q.attempt, attempt_seed: q.seed.toString(), segment_end: end, shuffle_seed: seed.toString(), new_quiz_id: newQuiz })
		]).results[0];
	}

	raw(type: string, fields: Record<string, unknown>) {
		return this.send([this.op(type, fields)]).results[0];
	}
}
