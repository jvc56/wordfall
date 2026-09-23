// The export formatter (PLAN.md § Exporting words), held byte for byte to
// contract-fixtures/export/ with the Rust one (backend/src/export). It yields
// the file in chunks, so a 300,000-question list is never one string.
import { leaveValueText } from '$lib/cascade/leave';

export type Which = 'all' | 'correct' | 'missed' | 'ungraded';
export type Column = 'question' | 'answer' | 'definition' | 'hooks' | 'grade';
export type Grade = 'correct' | 'missed';

export interface ExportWord {
	word: string;
	definition?: string;
	front_hooks?: string;
	back_hooks?: string;
}

export interface ExportQuestion {
	idx: number;
	key: string;
	/** Anagram. */
	words?: ExportWord[];
	/** Definition. */
	definition?: string;
	/** Leave Value. */
	value?: number;
	/** A Definition question's own hooks. */
	front_hooks?: string;
	back_hooks?: string;
}

export interface ExportQuiz {
	level: number;
	active: boolean;
	/** Question indexes in study order. */
	order: number[];
	grades: Record<string, Grade>;
	/** Which quiz a quiz export names, when two share a level (one active, one in the Trash). */
	pick?: boolean;
}

export interface Choices {
	scope: 'cascade' | 'quiz';
	level?: number;
	which: Which;
	format: 'txt' | 'csv';
	lines?: 'answers' | 'questions';
	columns?: Column[];
	order: 'study' | 'alphabetical';
	decimals: number;
}

export interface ExportInput {
	name: string;
	quiz_type: 'anagram' | 'definition' | 'leave_value';
	/** The distribution's tiles in tile order. */
	tiles: string[];
	/** In search order. */
	questions: ExportQuestion[];
	quizzes: ExportQuiz[];
	choices: Choices;
}

/** MAGPIE notation: a multi-character tile is written in brackets. */
export function splitTiles(s: string): string[] {
	const out: string[] = [];
	const cs = Array.from(s);
	for (let i = 0; i < cs.length; i++) {
		if (cs[i] === '[') {
			const j = cs.indexOf(']', i);
			out.push(cs.slice(i, j + 1).join(''));
			i = j;
		} else out.push(cs[i]);
	}
	return out;
}

function tileKey(key: string, tiles: string[]): number[] {
	return splitTiles(key).map((t) => tiles.indexOf(t.startsWith('[') ? t.slice(1, -1) : t));
}

function compare(a: number[], b: number[]): number {
	for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] - b[i];
	return a.length - b.length;
}

/** For a quiz, its grades; for a cascade, the union over its active quizzes (missed wins). */
export function gradesFor(input: ExportInput): { grades: Map<number, Grade>; quiz: ExportQuiz | null } {
	const ch = input.choices;
	if (ch.scope === 'quiz') {
		const quiz = input.quizzes.find((q) => q.level === ch.level && q.pick !== false)!;
		return { grades: new Map(Object.entries(quiz.grades).map(([k, g]) => [Number(k), g])), quiz };
	}
	const grades = new Map<number, Grade>();
	for (const q of input.quizzes) {
		if (!q.active) continue;
		for (const [k, g] of Object.entries(q.grades)) {
			const i = Number(k);
			grades.set(i, g === 'missed' || grades.get(i) === 'missed' ? 'missed' : 'correct');
		}
	}
	return { grades, quiz: null };
}

/** The selected questions in file order, with their grades. */
export function selection(input: ExportInput): { q: ExportQuestion; g: Grade | undefined }[] {
	const byIdx = new Map(input.questions.map((q) => [q.idx, q]));
	const { grades, quiz } = gradesFor(input);
	let idxs: number[];
	if (quiz) {
		idxs = [...quiz.order];
		if (input.choices.order === 'alphabetical') {
			const keys = new Map(idxs.map((i) => [i, tileKey(byIdx.get(i)!.key, input.tiles)]));
			idxs.sort((a, b) => compare(keys.get(a)!, keys.get(b)!));
		}
	} else {
		idxs = input.questions.map((q) => q.idx);
	}
	const which = input.choices.which;
	const out: { q: ExportQuestion; g: Grade | undefined }[] = [];
	for (const i of idxs) {
		const g = grades.get(i);
		if (which === 'all' || (which === 'ungraded' && g === undefined) || g === which) out.push({ q: byIdx.get(i)!, g });
	}
	return out;
}

function csvField(s: string): string {
	return /[,"\r\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
}

const hooksPair = (front?: string, back?: string) => `${splitTiles(front ?? '').join(' ')}|${splitTiles(back ?? '').join(' ')}`;

function cell(input: ExportInput, q: ExportQuestion, g: Grade | undefined, col: Column): string {
	const t = input.quiz_type;
	const d = input.choices.decimals;
	switch (col) {
		case 'question':
			return q.key;
		case 'grade':
			return g ?? '';
		case 'answer':
			if (t === 'anagram') return q.words!.map((w) => w.word).join(' ');
			if (t === 'definition') return q.definition!;
			return leaveValueText(q.value!, d, false);
		case 'definition':
			if (t === 'anagram') return q.words!.map((w) => w.definition ?? '').join(' | ');
			if (t === 'definition') return q.definition!;
			return '';
		case 'hooks':
			if (t === 'anagram') return q.words!.map((w) => hooksPair(w.front_hooks, w.back_hooks)).join(' / ');
			if (t === 'definition') return hooksPair(q.front_hooks, q.back_hooks);
			return '';
	}
}

/** The file, in chunks of about `chunk` entries. */
export function* formatExport(input: ExportInput, chunk = 5000): Generator<string> {
	const ch = input.choices;
	const entries = selection(input);
	let buf: string[] = [];
	const flush = function* () {
		if (buf.length) yield buf.join('');
		buf = [];
	};
	if (ch.format === 'txt') {
		for (const { q } of entries) {
			if (ch.lines === 'questions') buf.push(q.key + '\n');
			else if (input.quiz_type === 'anagram') for (const w of q.words!) buf.push(w.word + '\n');
			else if (input.quiz_type === 'definition') buf.push(q.definition! + '\n');
			else buf.push(leaveValueText(q.value!, ch.decimals, false) + '\n');
			if (buf.length >= chunk) yield* flush();
		}
		yield* flush();
		return;
	}
	const cols = ch.columns!;
	buf.push(cols.join(',') + '\r\n');
	for (const { q, g } of entries) {
		buf.push(cols.map((c) => csvField(cell(input, q, g, c))).join(',') + '\r\n');
		if (buf.length >= chunk) yield* flush();
	}
	yield* flush();
}

/**
 * `CSW24 7s - L2 missed.txt`: the cascade name, ` - L<level>` for a quiz,
 * the selection when not `all`; every Unicode scalar value outside A–Z, a–z,
 * 0–9, space, `.`, `_` and `-` becomes one `_`, cut to 100 before the extension.
 */
export function exportFilename(name: string, c: Pick<Choices, 'scope' | 'level' | 'which' | 'format'>): string {
	let base = name;
	if (c.scope === 'quiz') base += ` - L${c.level}`;
	if (c.which !== 'all') base += ` ${c.which}`;
	const safe = Array.from(base)
		.map((ch) => (/^[A-Za-z0-9 ._-]$/.test(ch) ? ch : '_'))
		.join('');
	return `${Array.from(safe).slice(0, 100).join('')}.${c.format}`;
}
