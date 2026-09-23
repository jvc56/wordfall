// An export built on the device from its view of the local rows (PLAN.md
// § Exporting words → How it is produced): questions from the `questions`
// store, answers, definitions and hooks from the `cards` store, grades and
// study order from the overlay-over-base rows. When something the export
// needs is not here, `localInput` says what, and the dialog goes to the
// server — except that a questions-only export needs only the keys.
import { APPLY_STORES } from '$lib/local/apply';
import type { UserDb } from '$lib/local/db';
import type { CardRow, CascadeRow, QuizRow } from '$lib/local/rows';
import * as view from '$lib/local/view';
import type { RwTx } from '$lib/local/view';
import { questionsHash } from '$lib/cascade/order';
import type { Column, ExportInput, ExportQuestion, ExportQuiz, Grade, Which } from './format';

export interface DeviceChoices {
	scope: 'cascade' | 'quiz';
	quiz_id?: string;
	which: Which;
	format: 'txt' | 'csv';
	lines: 'answers' | 'questions';
	columns: Column[];
	order: 'study' | 'alphabetical';
	decimals: number;
}

export type Missing = 'keys' | 'cards' | 'definitions' | 'hooks' | 'rows' | 'distribution';

/** Whether the file needs answers, definitions or hooks, and which. */
export function needs(quizType: string, c: DeviceChoices): { answers: boolean; definitions: boolean; hooks: boolean } {
	if (c.format === 'txt') return { answers: c.lines === 'answers', definitions: false, hooks: false };
	const col = (x: Column) => c.columns.includes(x);
	return {
		answers: col('answer') || col('definition') || col('hooks'),
		definitions: quizType === 'anagram' && col('definition'),
		hooks: col('hooks')
	};
}

/** The server's query for the same file (§ API → GET /api/cascades/:id/export). */
export function serverChoices(quizType: string, c: DeviceChoices): Record<string, unknown> {
	const n = needs(quizType, c);
	const out: Record<string, unknown> = {
		scope: c.scope,
		which: c.which,
		format: c.format,
		order: c.order,
		decimals: c.decimals,
		definitions: n.definitions || (quizType === 'definition' && n.answers),
		hooks: n.hooks
	};
	if (c.scope === 'quiz') out.quiz_id = c.quiz_id;
	if (c.format === 'txt') out.lines = c.lines;
	else out.columns = c.columns.join(',');
	return out;
}

/** Rows complete with positions, and the set the quiz row's hash names. */
async function rowsWhole(tx: RwTx, q: QuizRow) {
	const rows = await view.questionsOf(tx, q.id);
	const ok =
		rows.length === q.question_count &&
		rows.every((r) => r.position !== null) &&
		questionsHash(rows.map((r) => r.question_idx)).toString() === q.questions_hash;
	return { rows, ok };
}

export async function localInput(
	db: UserDb,
	cascade: CascadeRow,
	c: DeviceChoices
): Promise<{ input: ExportInput } | { missing: Missing }> {
	const tx = db.transaction([...APPLY_STORES, 'questions', 'cards', 'distributions']) as unknown as RwTx;
	const keys = await tx.objectStore('questions').index('cascade_id').getAll(cascade.id);
	if (keys.length < cascade.question_count) return { missing: 'keys' };
	const n = needs(cascade.quiz_type, c);
	if (cascade.quiz_type === 'definition' && n.hooks) return { missing: 'hooks' };
	let cards: Map<number, CardRow> = new Map();
	if (n.answers) {
		const all = await tx.objectStore('cards').index('cascade_id').getAll(cascade.id);
		if (all.length < cascade.question_count) return { missing: 'cards' };
		if (n.definitions && all.some((x) => !x.definitions)) return { missing: 'definitions' };
		if (n.hooks && all.some((x) => !x.hooks)) return { missing: 'hooks' };
		cards = new Map(all.map((x) => [x.idx, x]));
	}
	let tiles: string[] = [];
	if (c.order === 'alphabetical') {
		const d = await tx.objectStore('distributions').get(cascade.letter_distribution);
		if (!d) return { missing: 'distribution' };
		tiles = d.tiles.map((t) => t.letter);
	}
	const quizzes: ExportQuiz[] = [];
	for (const q of await view.quizzesOf(tx, cascade.id)) {
		const named = c.scope === 'quiz' && q.id === c.quiz_id;
		if (!(named || (c.scope === 'cascade' && q.status === 'active'))) continue;
		const { rows, ok } = await rowsWhole(tx, q);
		if (!ok) return { missing: 'rows' };
		const grades: Record<string, Grade> = {};
		for (const r of rows) if (r.grade) grades[String(r.question_idx)] = r.grade;
		quizzes.push({
			level: q.level,
			active: q.status === 'active',
			order: [...rows].sort((a, b) => a.position! - b.position!).map((r) => r.question_idx),
			grades,
			pick: c.scope === 'cascade' || named
		});
	}
	const questions: ExportQuestion[] = keys
		.sort((a, b) => a.idx - b.idx)
		.map((k) => {
			const q: ExportQuestion = { idx: k.idx, key: k.key };
			const card = cards.get(k.idx);
			if (card) {
				if (cascade.quiz_type === 'anagram') q.words = card.answer as ExportQuestion['words'];
				else if (cascade.quiz_type === 'definition') q.definition = String(card.answer ?? '');
				else q.value = Number(card.answer);
			}
			return q;
		});
	const level = quizzes.find((q) => q.pick && c.scope === 'quiz')?.level;
	return {
		input: {
			name: cascade.name,
			quiz_type: cascade.quiz_type,
			tiles,
			questions,
			quizzes,
			choices: {
				scope: c.scope,
				level,
				which: c.which,
				format: c.format,
				lines: c.lines,
				columns: c.columns,
				order: c.scope === 'cascade' ? 'study' : c.order,
				decimals: c.decimals
			}
		}
	};
}

/** The dialog's live count: the questions the selection holds, from the `questions` store's side. */
export function questionCount(input: ExportInput, select: (i: ExportInput) => unknown[]): number {
	return select(input).length;
}
