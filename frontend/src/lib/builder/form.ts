// The builder's filter tree as edited (PLAN.md § Creating a cascade, § Groups,
// § Filter applicability by quiz type). Rows hold what the user typed; `toWire`
// turns them into the canonical wire form and reports row errors; flags mark
// rows a quiz-type change, a lexicon change or a load made invalid, so they are
// kept rather than deleted silently.

import {
	PARTS_OF_SPEECH,
	QUIZ_TYPE_LABELS,
	defaultRange,
	filterDef,
	isGroup,
	rangeError,
	type CeilingContext,
	type ConditionType,
	type LexiconInfo,
	type QuizType,
	type WireCondition,
	type WireGroup
} from '$lib/filters';
import { canonicalLeaveOrder, Distribution, TileError } from '$lib/tiles';

export const MAX_ROWS = 100;
export const MAX_GROUPS = 100;
export const MAX_DEPTH = 4;
export const MAX_WORD_LIST_ENTRIES = 300_000;
export const MAX_TEXT_CHARS = 500;

let nextKey = 1;
export const newKey = () => nextKey++;

export interface RowState {
	kind: 'row';
	key: number;
	type: ConditionType;
	negated: boolean;
	pattern: string;
	min: string;
	max: string;
	lax: boolean;
	tiles: string;
	lexicon: string;
	/** Canonical for `entriesType`. */
	entries: string[];
	entriesType: QuizType;
	partOfSpeech: string;
	text: string;
	leaveMin: string;
	leaveMax: string;
	/** Set when a change made the row invalid; cleared by editing it. */
	flag: string | null;
}

export interface GroupState {
	kind: 'group';
	key: number;
	op: 'and' | 'or';
	children: NodeState[];
}

export type NodeState = RowState | GroupState;

export function newRow(type: ConditionType, quizType: QuizType, ctx: CeilingContext | null): RowState {
	const def = filterDef(type);
	const r = defaultRange(def, ctx);
	return {
		kind: 'row',
		key: newKey(),
		type,
		negated: false,
		pattern: '',
		min: String(r.min),
		max: String(r.max),
		lax: true,
		tiles: '',
		lexicon: '',
		entries: [],
		entriesType: quizType,
		partOfSpeech: 'noun',
		text: '',
		leaveMin: '',
		leaveMax: '',
		flag: null
	};
}

export function newGroup(op: 'and' | 'or', children: NodeState[] = []): GroupState {
	return { kind: 'group', key: newKey(), op, children };
}

export function* rows(g: GroupState): Generator<RowState> {
	for (const c of g.children) {
		if (c.kind === 'row') yield c;
		else yield* rows(c);
	}
}

/** The path of a node: its child indexes from the top group. */
export function pathOf(root: GroupState, key: number): number[] | null {
	const walk = (g: GroupState, path: number[]): number[] | null => {
		for (let i = 0; i < g.children.length; i++) {
			const c = g.children[i];
			if (c.key === key) return [...path, i];
			if (c.kind === 'group') {
				const p = walk(c, [...path, i]);
				if (p) return p;
			}
		}
		return null;
	};
	return walk(root, []);
}

export function nodeAt(root: GroupState, path: number[]): NodeState | null {
	let n: NodeState = root;
	for (const i of path) {
		if (n.kind !== 'group' || !n.children[i]) return null;
		n = n.children[i];
	}
	return n;
}

/** "Row 3", counting rows in order from the top, for messages. */
export function rowLabel(root: GroupState, key: number): string {
	let i = 0;
	for (const r of rows(root)) {
		i++;
		if (r.key === key) return `Row ${i} (${filterDef(r.type).label})`;
	}
	return 'A row';
}

// ---------------------------------------------------------------------------
// In Word List entries
// ---------------------------------------------------------------------------

/** Typed lines to canonical entries for a quiz type: a leave is sorted into
 * tile order and a word is not. Unparseable lines are counted, not kept. */
export function canonicalEntries(lines: string[], dist: Distribution, quizType: QuizType): { entries: string[]; invalid: number } {
	const leave = quizType === 'leave_value';
	const out = new Set<string>();
	let invalid = 0;
	for (const raw of lines) {
		const line = raw.trim();
		if (!line) continue;
		try {
			const tiles = dist.parseTyped(line, leave);
			if (!validLength(tiles.length, quizType) || (leave && !withinBag(tiles, dist))) {
				invalid++;
				continue;
			}
			out.add(dist.toMagpie(leave ? canonicalLeaveOrder(tiles) : tiles));
		} catch {
			invalid++;
		}
	}
	// Stored as a set, read back in byte order (PQ-009).
	return { entries: [...out].sort(byteOrder), invalid };
}

function validLength(n: number, q: QuizType): boolean {
	return n >= 1 && n <= (q === 'leave_value' ? 6 : 15);
}

function withinBag(tiles: number[], dist: Distribution): boolean {
	const counts = new Map<number, number>();
	for (const t of tiles) counts.set(t, (counts.get(t) ?? 0) + 1);
	return [...counts].every(([t, n]) => n <= dist.tiles[t].count);
}

export function byteOrder(a: string, b: string): number {
	const ea = new TextEncoder().encode(a);
	const eb = new TextEncoder().encode(b);
	const n = Math.min(ea.length, eb.length);
	for (let i = 0; i < n; i++) if (ea[i] !== eb[i]) return ea[i] - eb[i];
	return ea.length - eb.length;
}

/**
 * How many stored entries are not valid for a quiz type and distribution:
 * the structural check the device can make (PQ-010). An entry canonical for
 * the other side of Leave Value is invalid here.
 */
export function countInvalid(entries: string[], dist: Distribution | null, quizType: QuizType): number {
	const leave = quizType === 'leave_value';
	let bad = 0;
	for (const e of entries) {
		if (!dist) continue;
		try {
			const tiles = dist.parseMagpie(e, leave);
			const canon = leave ? dist.toMagpie(canonicalLeaveOrder(tiles)) : e;
			if (!validLength(tiles.length, quizType) || canon !== e || (leave && !withinBag(tiles, dist))) bad++;
		} catch {
			bad++;
		}
	}
	return bad;
}

const crossesLeave = (a: QuizType, b: QuizType) => (a === 'leave_value') !== (b === 'leave_value');

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

export function applicabilityFlag(r: RowState, q: QuizType): string | null {
	const def = filterDef(r.type);
	return def.appliesTo.includes(q) ? null : `${def.label} does not apply to ${QUIZ_TYPE_LABELS[q]} quizzes.`;
}

const WORD_LIST_FLAG =
	'These entries were entered for another quiz type: a leave is sorted into tile order and a word is not. Enter the list again.';

/**
 * A quiz-type change keeps the rows that still apply and flags the ones that
 * don't; an In Word List row is flagged on a change to or from Leave Value.
 */
export function changeQuizType(root: GroupState, to: QuizType) {
	for (const r of rows(root)) {
		const flag = applicabilityFlag(r, to);
		if (flag) r.flag = flag;
		else if (r.type === 'in_word_list' && crossesLeave(r.entriesType, to)) r.flag = WORD_LIST_FLAG;
		else if (r.flag && (r.flag.includes('does not apply') || r.flag === WORD_LIST_FLAG)) r.flag = null;
	}
}

/** In Lexicon rows whose target is on another distribution, or gone, are flagged. */
export function changeLexicon(root: GroupState, lexicons: LexiconInfo[], current: string) {
	const cur = lexicons.find((l) => l.name === current);
	for (const r of rows(root)) {
		if (r.type !== 'in_lexicon' || !r.lexicon) continue;
		const target = lexicons.find((l) => l.name === r.lexicon);
		if (!target) r.flag = `There is no lexicon named ${r.lexicon} any more.`;
		else if (cur && target.letter_distribution !== cur.letter_distribution)
			r.flag = `${r.lexicon} uses another letter distribution than ${current}.`;
		else if (r.flag?.startsWith('There is no lexicon') || r.flag?.includes('another letter distribution'))
			r.flag = null;
	}
}

/** The first flagged row, named, which refuses Save Search…. */
export function saveBlocker(root: GroupState, rowErrors: Map<number, string>): string | null {
	for (const r of rows(root)) {
		if (r.flag) return `${rowLabel(root, r.key)} is flagged: ${r.flag}`;
		const e = rowErrors.get(r.key);
		if (e) return `${rowLabel(root, r.key)}: ${e}`;
	}
	return null;
}

// ---------------------------------------------------------------------------
// From and to the wire
// ---------------------------------------------------------------------------

/** Rebuilds the form from a saved or cascade tree. The spec's quiz type is
 * what the entries were canonicalised under. */
export function fromWire(g: WireGroup, specType: QuizType): GroupState {
	return newGroup(
		g.op,
		g.children.map((c) => {
			if (isGroup(c)) return fromWire(c, specType);
			const r = newRow(c.type, specType, null);
			r.negated = c.negated;
			switch (filterDef(c.type).kind) {
				case 'pattern':
					r.pattern = String(c.pattern);
					break;
				case 'range':
				case 'order':
					r.min = String(c.min);
					r.max = String(c.max);
					if (typeof c.lax === 'boolean') r.lax = c.lax;
					break;
				case 'consists_of':
					r.tiles = String(c.tiles);
					r.min = String(c.min);
					r.max = String(c.max);
					break;
				case 'tiles':
					r.tiles = String(c.tiles);
					break;
				case 'lexicon':
					r.lexicon = String(c.lexicon);
					break;
				case 'entries':
					r.entries = (c.entries as string[]).slice();
					r.entriesType = specType;
					break;
				case 'part_of_speech':
					r.partOfSpeech = String(c.part_of_speech);
					break;
				case 'text':
					r.text = String(c.text);
					break;
				case 'leave_value':
					r.leaveMin = c.min == null ? '' : String(c.min);
					r.leaveMax = c.max == null ? '' : String(c.max);
					break;
			}
			return r;
		})
	);
}

export interface WireResult {
	tree: WireGroup;
	/** Row and group errors by key. */
	errors: Map<number, string>;
	entries: number;
}

function intOf(s: string): number | null {
	return /^\s*\d+\s*$/.test(s) ? Number(s) : null;
}

function numOf(s: string): number | null | undefined {
	if (s.trim() === '') return null;
	const n = Number(s);
	return Number.isFinite(n) ? n : undefined;
}

/**
 * Converts the form to the canonical wire tree, reporting row errors as the
 * server would (the server validates again). `dist` is null until the
 * lexicon's distribution has loaded, when tile inputs cannot be checked.
 */
export function toWire(root: GroupState, quizType: QuizType, dist: Distribution | null, ctx: CeilingContext | null): WireResult {
	const errors = new Map<number, string>();
	const leave = quizType === 'leave_value';
	let groups = 0;
	let count = 0;
	let entries = 0;
	const tiles = (r: RowState, text: string): string | null => {
		if (!dist) return text;
		try {
			return dist.canonicalTiles(text, leave);
		} catch (e) {
			errors.set(r.key, e instanceof TileError ? e.message : String(e));
			return null;
		}
	};
	const walk = (g: GroupState, depth: number): WireGroup => {
		groups++;
		if (groups === MAX_GROUPS + 1) errors.set(g.key, `A search can hold at most ${MAX_GROUPS} groups.`);
		if (depth > MAX_DEPTH) errors.set(g.key, `Groups nest at most ${MAX_DEPTH} deep.`);
		if (g.children.length === 0) errors.set(g.key, 'A group must hold at least one row or group.');
		const children = g.children.map((c): WireGroup | WireCondition => {
			if (c.kind === 'group') return walk(c, depth + 1);
			count++;
			if (count === MAX_ROWS + 1) errors.set(c.key, `A search can hold at most ${MAX_ROWS} rows.`);
			return row(c);
		});
		return { op: g.op, children };
	};
	const row = (r: RowState): WireCondition => {
		const def = filterDef(r.type);
		const out: WireCondition = { type: r.type, negated: def.negatable && r.negated };
		if (r.flag) errors.set(r.key, r.flag);
		switch (def.kind) {
			case 'pattern': {
				let p = r.pattern;
				if (dist) {
					try {
						p = dist.canonicalPattern(r.pattern, leave);
					} catch (e) {
						errors.set(r.key, e instanceof TileError ? e.message : String(e));
					}
				}
				if (Array.from(p).length > MAX_TEXT_CHARS)
					errors.set(r.key, `This is longer than the ${MAX_TEXT_CHARS}-character limit.`);
				out.pattern = p;
				break;
			}
			case 'range':
			case 'order':
			case 'consists_of': {
				const min = intOf(r.min);
				const max = intOf(r.max);
				if (min === null || max === null) errors.set(r.key, 'Enter whole numbers.');
				else {
					const e = rangeError(def, min, max, ctx);
					if (e) errors.set(r.key, e);
				}
				if (def.kind === 'consists_of') out.tiles = tiles(r, r.tiles) ?? r.tiles;
				out.min = min ?? 0;
				out.max = max ?? 0;
				if (def.kind === 'order') out.lax = r.lax;
				break;
			}
			case 'tiles':
				out.tiles = tiles(r, r.tiles) ?? r.tiles;
				break;
			case 'lexicon':
				if (!r.lexicon) errors.set(r.key, 'Choose a lexicon.');
				out.lexicon = r.lexicon;
				break;
			case 'entries':
				entries += r.entries.length;
				if (entries > MAX_WORD_LIST_ENTRIES && entries - r.entries.length <= MAX_WORD_LIST_ENTRIES)
					errors.set(
						r.key,
						`In Word List rows can hold ${MAX_WORD_LIST_ENTRIES} entries in all; this row takes the total to ${entries}.`
					);
				out.entries = r.entries;
				break;
			case 'part_of_speech':
				if (!PARTS_OF_SPEECH.some(([v]) => v === r.partOfSpeech)) errors.set(r.key, 'Choose a part of speech.');
				out.part_of_speech = r.partOfSpeech;
				break;
			case 'text':
				if (!r.text) errors.set(r.key, 'Enter some text.');
				else if (Array.from(r.text).length > MAX_TEXT_CHARS)
					errors.set(r.key, `This is longer than the ${MAX_TEXT_CHARS}-character limit.`);
				out.text = r.text;
				break;
			case 'none':
				break;
			case 'leave_value': {
				const min = numOf(r.leaveMin);
				const max = numOf(r.leaveMax);
				if (min === undefined || max === undefined) errors.set(r.key, 'Enter numbers, or leave a bound blank.');
				else if (min === null && max === null) errors.set(r.key, 'Fill in at least one bound.');
				else if (min !== null && max !== null && min > max) errors.set(r.key, 'The minimum is above the maximum.');
				out.min = min ?? null;
				out.max = max ?? null;
				break;
			}
		}
		return out;
	};
	const tree = walk(root, 1);
	return { tree, errors, entries };
}
