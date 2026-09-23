// The frontend filter table (PLAN.md § Frontend → FilterRow, § Filter
// reference). It drives the UI only; the backend validates on its own, and a
// contract test keeps the two in step through contract-fixtures/filters/.

export type QuizType = 'anagram' | 'definition' | 'leave_value';
export const QUIZ_TYPES: QuizType[] = ['anagram', 'definition', 'leave_value'];
export const QUIZ_TYPE_LABELS: Record<QuizType, string> = {
	anagram: 'Anagram',
	definition: 'Definition',
	leave_value: 'Leave Value'
};

export type ConditionType =
	| 'anagram_match'
	| 'pattern_match'
	| 'subanagram_match'
	| 'length'
	| 'in_lexicon'
	| 'in_word_list'
	| 'num_vowels'
	| 'includes_letters'
	| 'probability_order'
	| 'limit_by_probability_order'
	| 'playability_order'
	| 'limit_by_playability_order'
	| 'num_unique_letters'
	| 'point_value'
	| 'takes_prefix'
	| 'takes_suffix'
	| 'part_of_speech'
	| 'definition'
	| 'consists_of'
	| 'num_anagrams'
	| 'front_inner_hook'
	| 'back_inner_hook'
	| 'leave_value';

export type ParamKind =
	| 'pattern'
	| 'range'
	| 'order'
	| 'consists_of'
	| 'tiles'
	| 'lexicon'
	| 'entries'
	| 'part_of_speech'
	| 'text'
	| 'none'
	| 'leave_value';

/** What a row's ceiling is measured against. */
export type CeilingKind =
	| 'tiles' // 15, or 6 on a Leave Value cascade
	| 'percent' // 100
	| 'point_value' // 15 × (or 6 ×) the highest tile value
	| 'num_anagrams' // the target's largest num_anagrams
	| 'order_rank' // the target's largest length (size) bucket
	| 'target_size'; // word_count or leave_count

export interface FilterDef {
	type: ConditionType;
	label: string;
	/** Wire fields, in the parameter table's order. */
	fields: string[];
	kind: ParamKind;
	negatable: boolean;
	limit: boolean;
	appliesTo: QuizType[];
	floor?: number;
	ceiling?: CeilingKind;
	help?: string;
}

const ALL: QuizType[] = ['anagram', 'definition', 'leave_value'];
const WORDS: QuizType[] = ['anagram', 'definition'];

export const FILTERS: FilterDef[] = [
	{ type: 'anagram_match', label: 'Anagram Match', fields: ['pattern'], kind: 'pattern', negatable: true, limit: false, appliesTo: ALL,
		help: 'The word uses exactly these tiles, in any order. `.` is any one tile, `[AB]` one of a set, `*` any extra tiles.' },
	{ type: 'pattern_match', label: 'Pattern Match', fields: ['pattern'], kind: 'pattern', negatable: true, limit: false, appliesTo: ALL,
		help: 'The word matches left to right. `.` is any one tile, `*` any run of tiles, `[AB]` one of a set.' },
	{ type: 'subanagram_match', label: 'Subanagram Match', fields: ['pattern'], kind: 'pattern', negatable: true, limit: false, appliesTo: ALL,
		help: 'Every tile of the word can be taken from these tiles.' },
	{ type: 'length', label: 'Length', fields: ['min', 'max'], kind: 'range', negatable: false, limit: false, appliesTo: ALL, floor: 1, ceiling: 'tiles' },
	{ type: 'in_lexicon', label: 'In Lexicon', fields: ['lexicon'], kind: 'lexicon', negatable: true, limit: false, appliesTo: WORDS,
		help: 'The word is also valid in another lexicon on the same letter distribution.' },
	{ type: 'in_word_list', label: 'In Word List', fields: ['entries'], kind: 'entries', negatable: true, limit: false, appliesTo: ALL,
		help: 'Paste or upload a list, one entry per line. Entries not valid in the lexicon are ignored.' },
	{ type: 'num_vowels', label: 'Number of Vowels', fields: ['min', 'max'], kind: 'range', negatable: false, limit: false, appliesTo: ALL, floor: 0, ceiling: 'tiles' },
	{ type: 'includes_letters', label: 'Includes Letters', fields: ['tiles'], kind: 'tiles', negatable: true, limit: false, appliesTo: ALL,
		help: 'Each tile appears at least as often as here. Not matches words missing any of them: to exclude every tile of a set, add one Not row per tile.' },
	{ type: 'probability_order', label: 'Probability Order', fields: ['min', 'max', 'lax'], kind: 'order', negatable: false, limit: false, appliesTo: ALL, floor: 1, ceiling: 'order_rank' },
	{ type: 'limit_by_probability_order', label: 'Limit by Probability Order', fields: ['min', 'max', 'lax'], kind: 'order', negatable: false, limit: true, appliesTo: ALL, floor: 1, ceiling: 'target_size' },
	{ type: 'playability_order', label: 'Playability Order', fields: ['min', 'max', 'lax'], kind: 'order', negatable: false, limit: false, appliesTo: WORDS, floor: 1, ceiling: 'order_rank' },
	{ type: 'limit_by_playability_order', label: 'Limit by Playability Order', fields: ['min', 'max', 'lax'], kind: 'order', negatable: false, limit: true, appliesTo: WORDS, floor: 1, ceiling: 'target_size' },
	{ type: 'num_unique_letters', label: 'Number of Unique Letters', fields: ['min', 'max'], kind: 'range', negatable: false, limit: false, appliesTo: ALL, floor: 0, ceiling: 'tiles' },
	{ type: 'point_value', label: 'Point Value', fields: ['min', 'max'], kind: 'range', negatable: false, limit: false, appliesTo: ALL, floor: 0, ceiling: 'point_value' },
	{ type: 'takes_prefix', label: 'Takes Prefix', fields: ['tiles'], kind: 'tiles', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'takes_suffix', label: 'Takes Suffix', fields: ['tiles'], kind: 'tiles', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'part_of_speech', label: 'Part of Speech', fields: ['part_of_speech'], kind: 'part_of_speech', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'definition', label: 'Definition', fields: ['text'], kind: 'text', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'consists_of', label: 'Consists of', fields: ['tiles', 'min', 'max'], kind: 'consists_of', negatable: false, limit: false, appliesTo: ALL, floor: 0, ceiling: 'percent' },
	{ type: 'num_anagrams', label: 'Number of Anagrams', fields: ['min', 'max'], kind: 'range', negatable: false, limit: false, appliesTo: ALL, floor: 0, ceiling: 'num_anagrams' },
	{ type: 'front_inner_hook', label: 'Front Inner Hook', fields: [], kind: 'none', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'back_inner_hook', label: 'Back Inner Hook', fields: [], kind: 'none', negatable: true, limit: false, appliesTo: WORDS },
	{ type: 'leave_value', label: 'Leave Value', fields: ['min', 'max'], kind: 'leave_value', negatable: false, limit: false, appliesTo: ['leave_value'] }
];

export const FILTER_BY_TYPE = new Map(FILTERS.map((f) => [f.type, f]));

export function filterDef(t: ConditionType): FilterDef {
	const f = FILTER_BY_TYPE.get(t);
	if (!f) throw new Error(`unknown condition type ${t}`);
	return f;
}

export const PARTS_OF_SPEECH = [
	['adjective', 'Adjective'],
	['adverb', 'Adverb'],
	['conjunction', 'Conjunction'],
	['definite_article', 'Definite Article'],
	['indefinite_article', 'Indefinite Article'],
	['interjection', 'Interjection'],
	['noun', 'Noun'],
	['preposition', 'Preposition'],
	['pronoun', 'Pronoun'],
	['verb', 'Verb']
] as const;

/** `GET /api/lexicons` (PLAN.md § API → Catalog and search). */
export interface LexiconInfo {
	name: string;
	letter_distribution: string;
	word_count: number;
	leave_count: number | null;
	max_num_anagrams: number;
	max_order_rank: number;
	max_leave_num_anagrams: number | null;
	max_leave_order_rank: number | null;
}

export interface CeilingContext {
	quizType: QuizType;
	lexicon: LexiconInfo;
	/** The highest tile value in the lexicon's distribution. */
	maxTileValue: number;
}

/** A row's ceiling: the largest value a bound may take. */
export function ceilingOf(def: FilterDef, ctx: CeilingContext): number | null {
	const leave = ctx.quizType === 'leave_value';
	const tiles = leave ? 6 : 15;
	switch (def.ceiling) {
		case 'tiles':
			return tiles;
		case 'percent':
			return 100;
		case 'point_value':
			return tiles * ctx.maxTileValue;
		case 'num_anagrams':
			return leave ? (ctx.lexicon.max_leave_num_anagrams ?? 0) : ctx.lexicon.max_num_anagrams;
		case 'order_rank':
			return leave ? (ctx.lexicon.max_leave_order_rank ?? 0) : ctx.lexicon.max_order_rank;
		case 'target_size':
			return leave ? (ctx.lexicon.leave_count ?? 0) : ctx.lexicon.word_count;
		default:
			return null;
	}
}

/** A new integer range row starts at its floor and its ceiling. */
export function defaultRange(def: FilterDef, ctx: CeilingContext | null): { min: number; max: number } {
	const floor = def.floor ?? 0;
	return { min: floor, max: (ctx && ceilingOf(def, ctx)) ?? floor };
}

/**
 * The client's range check, the server's in miniature: min ≤ max, bounds
 * within floor and ceiling (naming the ceiling), and a range that narrows
 * something. Returns a message for the row, or null.
 */
export function rangeError(def: FilterDef, min: number, max: number, ctx: CeilingContext | null): string | null {
	const floor = def.floor ?? 0;
	const ceiling = ctx ? ceilingOf(def, ctx) : null;
	if (!Number.isInteger(min) || !Number.isInteger(max)) return 'Enter whole numbers.';
	if (min < floor) return `The minimum must be at least ${floor}.`;
	if (ceiling !== null && max > ceiling) return `The maximum can be at most ${ceiling}.`;
	if (ceiling !== null && min > ceiling) return `The minimum can be at most ${ceiling}.`;
	if (min > max) return 'The minimum is above the maximum.';
	if (ceiling !== null && min === floor && max === ceiling)
		return `${floor}–${ceiling} includes everything, so this row narrows nothing.`;
	return null;
}

// ---------------------------------------------------------------------------
// Wire form
// ---------------------------------------------------------------------------

export interface WireCondition {
	type: ConditionType;
	negated: boolean;
	[field: string]: unknown;
}

export interface WireGroup {
	op: 'and' | 'or';
	children: WireNode[];
}

export type WireNode = WireGroup | WireCondition;

export function isGroup(n: WireNode): n is WireGroup {
	return 'op' in n;
}

/** Serialises a condition with its keys in the wire order: type, negated, fields. */
export function conditionJson(c: WireCondition): string {
	const def = filterDef(c.type);
	const o: Record<string, unknown> = { type: c.type, negated: c.negated };
	for (const f of def.fields) o[f] = c[f];
	return JSON.stringify(o);
}

export function treeJson(g: WireGroup): string {
	const kids = g.children.map((c) => (isGroup(c) ? treeJson(c) : conditionJson(c)));
	return `{"op":${JSON.stringify(g.op)},"children":[${kids.join(',')}]}`;
}

export class WireError extends Error {}

/** Reads a condition from JSON, refusing unknown types and extra fields. */
export function parseCondition(v: unknown): WireCondition {
	if (typeof v !== 'object' || v === null) throw new WireError('a condition must be an object');
	const o = v as Record<string, unknown>;
	const def = FILTER_BY_TYPE.get(o.type as ConditionType);
	if (!def) throw new WireError(`unknown condition type ${String(o.type)}`);
	for (const k of Object.keys(o)) {
		if (k !== 'type' && k !== 'negated' && !def.fields.includes(k))
			throw new WireError(`unexpected field ${k} for ${def.type}`);
	}
	if (typeof o.negated !== 'boolean') throw new WireError('negated must be a boolean');
	for (const f of def.fields) if (!(f in o)) throw new WireError(`${f} is required`);
	return o as WireCondition;
}

export function parseTree(v: unknown): WireGroup {
	if (typeof v !== 'object' || v === null) throw new WireError('a group must be an object');
	const o = v as Record<string, unknown>;
	if (o.op !== 'and' && o.op !== 'or') throw new WireError('op must be "and" or "or"');
	for (const k of Object.keys(o)) if (k !== 'op' && k !== 'children') throw new WireError(`unexpected field ${k}`);
	if (!Array.isArray(o.children)) throw new WireError('children must be an array');
	return {
		op: o.op,
		children: o.children.map((c) =>
			typeof c === 'object' && c !== null && 'op' in c ? parseTree(c) : parseCondition(c)
		)
	};
}

// ---------------------------------------------------------------------------
// The default cascade name
// ---------------------------------------------------------------------------

export const MAX_NAME = 200;

/** `. W * M . S` reads `.W*M.S`, as Zyzzyva users write it. */
function compactPattern(p: string): string {
	return p.replace(/\[([^\]]*)\]/g, (_, inner: string) => `[${inner.split(' ').join('')}]`).split(' ').join('');
}

function num(n: unknown): string {
	return String(n);
}

/** One row as the summary writes it. */
export function describe(c: WireCondition): string {
	const def = filterDef(c.type);
	const not = c.negated ? 'Not ' : '';
	let body: string;
	switch (def.kind) {
		case 'pattern':
			body = `${def.label} ${compactPattern(String(c.pattern))}`;
			break;
		case 'range':
			body = `${def.label} ${num(c.min)}–${num(c.max)}`;
			break;
		case 'order':
			body = `${def.label} ${num(c.min)}–${num(c.max)}${c.lax ? '' : ' strict'}`;
			break;
		case 'consists_of':
			body = `${def.label} ${String(c.tiles)} ${num(c.min)}–${num(c.max)}%`;
			break;
		case 'tiles':
			body = `${def.label} ${String(c.tiles)}`;
			break;
		case 'lexicon':
			body = `${def.label} ${String(c.lexicon)}`;
			break;
		case 'entries': {
			const n = (c.entries as unknown[]).length;
			body = `${def.label} (${num(n)} ${n === 1 ? 'entry' : 'entries'})`;
			break;
		}
		case 'part_of_speech': {
			const label = PARTS_OF_SPEECH.find(([v]) => v === c.part_of_speech)?.[1] ?? String(c.part_of_speech);
			body = `${def.label} ${label}`;
			break;
		}
		case 'text':
			body = `${def.label} “${String(c.text)}”`;
			break;
		case 'none':
			body = def.label;
			break;
		case 'leave_value': {
			const min = c.min as number | null;
			const max = c.max as number | null;
			body =
				min !== null && max !== null
					? `${def.label} ${min}–${max}`
					: min !== null
						? `${def.label} ≥ ${min}`
						: `${def.label} ≤ ${max}`;
			break;
		}
	}
	return not + body;
}

function describeGroup(g: WireGroup, top: boolean): string {
	const parts = g.children.map((c) => (isGroup(c) ? describeGroup(c, false) : describe(c)));
	if (top && g.op === 'and') return parts.join(' · ');
	const joined = parts.join(g.op === 'and' ? ' and ' : ' or ');
	return top ? joined : `(${joined})`;
}

/** Cuts to the first 199 Unicode scalar values and `…` when over 200. */
export function cutName(name: string): string {
	const cs = Array.from(name);
	return cs.length > MAX_NAME ? cs.slice(0, MAX_NAME - 1).join('') + '…' : name;
}

/** `CSW24 · Length 7–7 · Probability Order 1–1,000`, built on the device only. */
export function summaryName(lexicon: string, tree: WireGroup): string {
	const body = tree.children.length ? describeGroup(tree, true) : '';
	return cutName(body ? `${lexicon} · ${body}` : lexicon);
}
