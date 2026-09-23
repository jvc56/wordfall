// Typed mode for Anagram quizzes (PLAN.md § Taking a quiz → Typed mode). The
// input is upper-cased, trimmed and converted to tiles; a word that is one of
// the answers joins the found list, one already entered is ignored, one with
// the wrong tile count is left in the input ("type one word at a time"), and
// anything else is wrong. With alphabetical order on, an answer that sorts
// before the furthest answer entered so far is still found but marked out of
// order and counted as a wrong entry.
import { compareTiles, type Distribution } from '$lib/tiles';

export interface TypedState {
	/** Found answers (MAGPIE notation), in alphabetical order. */
	found: string[];
	/** Found answers entered out of order. */
	outOfOrder: string[];
	/** Wrong entries, as typed. */
	wrong: string[];
	/** The furthest answer entered so far, for alphabetical order. */
	furthest: number[] | null;
}

export type Submit =
	| { kind: 'found'; word: string }
	| { kind: 'out_of_order'; word: string }
	| { kind: 'already' }
	| { kind: 'one_word' }
	| { kind: 'wrong' }
	/** Enter on an empty input: Show / Next. */
	| { kind: 'empty' };

export function emptyTyped(): TypedState {
	return { found: [], outOfOrder: [], wrong: [], furthest: null };
}

const same = (a: number[], b: number[]) => a.length === b.length && a.every((t, i) => t === b[i]);

/** Enter pressed with `text` in the input; returns the new state and what happened. */
export function submitTyped(
	state: TypedState,
	text: string,
	answers: string[],
	questionTiles: number,
	dist: Distribution,
	alphabetical: boolean
): { state: TypedState; result: Submit } {
	const trimmed = text.trim().toUpperCase();
	if (!trimmed) return { state, result: { kind: 'empty' } };
	let tiles: number[];
	try {
		tiles = dist.parseTyped(trimmed, false);
	} catch {
		return { state: { ...state, wrong: [...state.wrong, trimmed] }, result: { kind: 'wrong' } };
	}
	if (tiles.length !== questionTiles) return { state, result: { kind: 'one_word' } };
	const answer = answers.find((a) => same(dist.parseMagpie(a, false), tiles));
	if (!answer) return { state: { ...state, wrong: [...state.wrong, trimmed] }, result: { kind: 'wrong' } };
	if (state.found.includes(answer)) return { state, result: { kind: 'already' } };
	const found = [...state.found, answer].sort((a, b) => compareTiles(dist.parseMagpie(a, false), dist.parseMagpie(b, false)));
	if (alphabetical && state.furthest && compareTiles(tiles, state.furthest) < 0) {
		return { state: { ...state, found, outOfOrder: [...state.outOfOrder, answer] }, result: { kind: 'out_of_order', word: answer } };
	}
	const furthest = !state.furthest || compareTiles(tiles, state.furthest) > 0 ? tiles : state.furthest;
	return { state: { ...state, found, furthest }, result: { kind: 'found', word: answer } };
}

/** Every anagram found. */
export function allFound(state: TypedState, answers: string[]): boolean {
	return answers.every((a) => state.found.includes(a));
}

/** Correct only if every anagram was found with no wrong entries (out-of-order ones count as wrong). */
export function typedGrade(state: TypedState, answers: string[]): 'correct' | 'missed' {
	return allFound(state, answers) && state.wrong.length === 0 && state.outOfOrder.length === 0 ? 'correct' : 'missed';
}
