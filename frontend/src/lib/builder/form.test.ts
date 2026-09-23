// PLAN.md § Unit tests → Frontend: the In Word List row across a quiz-type
// change and on the load path, and Save Search… refused while it is flagged.
import { describe, expect, it } from 'vitest';
import { Distribution, type TileDef } from '$lib/tiles';
import {
	canonicalEntries,
	changeQuizType,
	countInvalid,
	fromWire,
	newGroup,
	newRow,
	saveBlocker,
	toWire
} from './form';

const letters = '?ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const tiles: TileDef[] = Array.from(letters).map((l, i) => ({
	letter: l,
	blank_letter: i === 0 ? '?' : l.toLowerCase(),
	count: i === 0 ? 2 : 4,
	value: 1,
	is_vowel: 'AEIOU'.includes(l)
}));
const en = new Distribution('english', tiles);

function wordListForm(lines: string[], q: 'anagram' | 'definition' | 'leave_value') {
	const row = newRow('in_word_list', q, null);
	const { entries } = canonicalEntries(lines, en, q);
	row.entries = entries;
	row.entriesType = q;
	return { root: newGroup('and', [row]), row };
}

describe('In Word List across a quiz-type change', () => {
	it('a leave list is flagged and recounted when the type becomes Anagram', () => {
		const { root, row } = wordListForm(['seir?', 'TA', 'eeiors'], 'leave_value');
		expect(row.entries).toEqual(['?EIRS', 'AT', 'EEIORS']);
		expect(countInvalid(row.entries, en, 'leave_value')).toBe(0);
		changeQuizType(root, 'anagram');
		expect(row.flag).toMatch(/another quiz type/);
		expect(countInvalid(row.entries, en, 'anagram')).toBe(1); // the blank
	});

	it('a list of 7-tile words is flagged and recounted when it becomes Leave Value', () => {
		const { root, row } = wordListForm(['RETAINS', 'NASTIER'], 'anagram');
		changeQuizType(root, 'leave_value');
		expect(row.flag).not.toBeNull();
		expect(countInvalid(row.entries, en, 'leave_value')).toBe(2);
	});

	it('Anagram ↔ Definition flags nothing and leaves the count alone', () => {
		const { root, row } = wordListForm(['RETAINS', 'QI'], 'anagram');
		changeQuizType(root, 'definition');
		expect(row.flag).toBeNull();
		expect(countInvalid(row.entries, en, 'definition')).toBe(0);
		changeQuizType(root, 'anagram');
		expect(row.flag).toBeNull();
	});

	it('a saved search whose spec records leave_value is flagged the moment it loads into Anagram', () => {
		const tree = { op: 'and' as const, children: [{ type: 'in_word_list' as const, negated: false, entries: ['?EIRS'] }] };
		const root = fromWire(tree, 'leave_value');
		changeQuizType(root, 'anagram');
		const [row] = root.children;
		expect(row.kind === 'row' && row.flag).toMatch(/another quiz type/);
	});

	it('Save Search… is refused while that row is flagged, naming the row', () => {
		const { root } = wordListForm(['?EIRS'], 'leave_value');
		root.children.unshift(newRow('length', 'anagram', null));
		changeQuizType(root, 'anagram');
		const { errors } = toWire(root, 'anagram', en, null);
		expect(saveBlocker(root, errors)).toMatch(/^Row 2 \(In Word List\) is flagged/);
	});

	it('flags rows that no longer apply instead of deleting them', () => {
		const root = newGroup('and', [newRow('definition', 'anagram', null), newRow('length', 'anagram', null)]);
		changeQuizType(root, 'leave_value');
		expect(root.children).toHaveLength(2);
		expect(root.children[0].kind === 'row' && root.children[0].flag).toMatch(/does not apply to Leave Value/);
		changeQuizType(root, 'anagram');
		expect(root.children[0].kind === 'row' && root.children[0].flag).toBeNull();
	});
});

describe('toWire', () => {
	it('canonicalises typed tiles and patterns and reports errors by row', () => {
		const p = newRow('pattern_match', 'anagram', null);
		p.pattern = '.w*m.s';
		const i = newRow('includes_letters', 'anagram', null);
		i.tiles = 'q?';
		const root = newGroup('and', [p, i, newGroup('or')]);
		const { tree, errors } = toWire(root, 'anagram', en, null);
		expect((tree.children[0] as unknown as { pattern: string }).pattern).toBe('. W * M . S');
		expect(errors.get(i.key)).toMatch(/use `.`/);
		expect(errors.get(root.children[2].key)).toMatch(/at least one row/);
	});
});
