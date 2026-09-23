// The TypeScript side of PLAN.md § Unit tests → Tile handling.
import { describe, expect, it } from 'vitest';
import { Distribution, TileError, compareTiles, magpieSyntaxError, type TileDef } from './tiles';

const t = (letter: string, blank: string, count: number, value: number, vowel = false): TileDef => ({
	letter,
	blank_letter: blank,
	count,
	value,
	is_vowel: vowel
});

const cat = new Distribution('cat', [
	t('?', '?', 2, 0),
	t('A', 'a', 12, 1, true),
	t('C', 'c', 3, 2),
	t('Ç', 'ç', 1, 10),
	t('E', 'e', 13, 1, true),
	t('L', 'l', 4, 1),
	t('L·L', 'l·l', 1, 10),
	t('N', 'n', 6, 1),
	t('NY', 'ny', 1, 10),
	t('QU', 'qu', 1, 8),
	t('S', 's', 8, 1),
	t('Y', 'y', 1, 4)
]);

describe('tiles', () => {
	it('parses A[NY]S and typed anys to three tiles', () => {
		const a = cat.parseMagpie('A[NY]S', false);
		expect(a).toHaveLength(3);
		expect(cat.parseTyped('anys', false)).toEqual(a);
		expect(cat.toMagpie(a)).toBe('A[NY]S');
		expect(cat.toDisplay(a)).toBe('ANYS');
	});

	it('keeps a space-separated N and Y as two tiles', () => {
		expect(cat.parseTyped('N Y', false)).toHaveLength(2);
		expect(cat.parseTyped('an ys', false)).toHaveLength(4);
		expect(cat.parseTyped('cel·la', false)).toHaveLength(4);
	});

	it('never backtracks', () => {
		expect(cat.parseTyped('que', false)).toHaveLength(2);
		expect(() => cat.parseTyped('qe', false)).toThrow(TileError);
	});

	it('tokenises patterns canonically', () => {
		expect(cat.canonicalPattern('N Y', false)).toBe('N Y');
		expect(cat.canonicalPattern('[A NY]', false)).toBe('[A NY]');
		expect(cat.canonicalPattern('[any]', false)).toBe('[A NY]');
		expect(cat.canonicalPattern('.w*m.s'.replace(/[wm]/g, 'a'), false)).toBe('. A * A . S');
		expect(cat.canonicalPattern('c.nya', false)).toBe('C . NY A');
		expect(() => cat.canonicalPattern('?a', false)).toThrow(/use `.`/);
		expect(cat.canonicalPattern('?a', true)).toBe('? A');
	});

	it('rejects malformed notation as MAGPIE does', () => {
		for (const bad of ['[', '[]', '[A]', '[[NY]]', 'A]', 'a']) {
			expect(() => cat.parseMagpie(bad, false), bad).toThrow(TileError);
		}
		expect(magpieSyntaxError('[A]', false)).toMatch(/without brackets/);
		expect(magpieSyntaxError('A?', false)).toMatch(/use `.`/);
		expect(magpieSyntaxError('A[NY]S', false)).toBeNull();
	});

	it('sorts in distribution order with the blank first', () => {
		expect(cat.canonicalLeave('s[l·l]?a')).toBe('?A[L·L]S');
		const la = cat.parseMagpie('LA', false);
		const ll = cat.parseMagpie('[L·L]', false);
		expect(compareTiles(la, ll)).toBeLessThan(0);
		expect(compareTiles(cat.parseMagpie('CA', false), cat.parseMagpie('CAS', false))).toBeLessThan(0);
	});
});
