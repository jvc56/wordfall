// PLAN.md § Contract fixtures → Contract test (the frontend side), the
// default-name summary, and the filter rows' ceilings (§ Unit tests → Frontend).
import { describe, expect, it } from 'vitest';
import fixture from '../../../contract-fixtures/filters/conditions.json';
import {
	FILTERS,
	ceilingOf,
	conditionJson,
	cutName,
	defaultRange,
	describe as describeRow,
	filterDef,
	parseCondition,
	rangeError,
	summaryName,
	type CeilingContext,
	type LexiconInfo,
	type WireCondition
} from './filters';

describe('the filter table against the shared fixture', () => {
	it('lists the same types, fields, Not, limits, applicability and labels', () => {
		expect(FILTERS.map((f) => f.type)).toEqual(fixture.types.map((t) => t.type));
		for (const t of fixture.types) {
			const f = filterDef(t.type as never);
			expect(f.fields).toEqual(t.fields);
			expect(f.negatable).toBe(t.negatable);
			expect(f.limit).toBe(t.limit);
			expect(f.appliesTo).toEqual(t.applies_to);
			expect(f.label).toBe(t.label);
		}
	});

	it('serialises every valid condition byte for byte', () => {
		for (const v of fixture.valid) {
			expect(conditionJson(parseCondition(v.condition))).toBe(v.text);
		}
	});

	it('refuses a condition with an extra field', () => {
		expect(() => parseCondition(fixture.extra_field)).toThrow(/unexpected field lax/);
	});
});

describe('the default name', () => {
	it('summarises every filter type', () => {
		const lines = fixture.valid.map((v) => describeRow(v.condition as WireCondition));
		expect(lines).toEqual([
			'Anagram Match AB.',
			'Pattern Match AB.',
			'Subanagram Match AB.',
			'Length 2–5',
			'In Lexicon EN-FIX',
			'In Word List (2 entries)',
			'Number of Vowels 2–5',
			'Includes Letters AB',
			'Probability Order 2–5',
			'Limit by Probability Order 2–5',
			'Playability Order 2–5',
			'Limit by Playability Order 2–5',
			'Number of Unique Letters 2–5',
			'Point Value 2–5',
			'Takes Prefix AB',
			'Takes Suffix AB',
			'Part of Speech Noun',
			'Definition “fish”',
			'Consists of AB 70–100%',
			'Number of Anagrams 2–5',
			'Front Inner Hook',
			'Back Inner Hook',
			'Leave Value ≥ 10.5'
		]);
		expect(describeRow({ type: 'includes_letters', negated: true, tiles: 'U' })).toBe('Not Includes Letters U');
		expect(describeRow({ type: 'probability_order', negated: false, min: 1, max: 1000, lax: false })).toBe(
			'Probability Order 1–1000 strict'
		);
	});

	it('writes groups with parentheses', () => {
		const tree = {
			op: 'and' as const,
			children: [
				{ type: 'length' as const, negated: false, min: 7, max: 7 },
				{
					op: 'or' as const,
					children: [
						{ type: 'includes_letters' as const, negated: false, tiles: 'Q' },
						{ type: 'includes_letters' as const, negated: false, tiles: 'Z' }
					]
				}
			]
		};
		expect(summaryName('CSW24', tree)).toBe('CSW24 · Length 7–7 · (Includes Letters Q or Includes Letters Z)');
		expect(
			summaryName('CSW24', {
				op: 'and',
				children: [
					{ type: 'length', negated: false, min: 7, max: 7 },
					{ type: 'probability_order', negated: false, min: 1, max: 1000, lax: true }
				]
			})
		).toBe('CSW24 · Length 7–7 · Probability Order 1–1000');
	});

	it('cuts a long name at exactly 199 scalar values plus …', () => {
		const long = '𝒜'.repeat(250);
		const cut = cutName(long);
		expect(Array.from(cut)).toHaveLength(200);
		expect(cut.endsWith('…')).toBe(true);
		expect(Array.from(cut.slice(0, -1))).toHaveLength(199);
		expect(cutName('x'.repeat(200))).toBe('x'.repeat(200));
	});
});

describe('ceilings', () => {
	// A stubbed GET /api/lexicons row and distribution.
	const lexicon: LexiconInfo = {
		name: 'L',
		letter_distribution: 'english',
		word_count: 280_000,
		leave_count: 900_000,
		max_num_anagrams: 13,
		max_order_rank: 40_000,
		max_leave_num_anagrams: 9,
		max_leave_order_rank: 300_000
	};
	const ctx: CeilingContext = { quizType: 'anagram', lexicon, maxTileValue: 10 };

	it('opens each row at its floor and its own ceiling, refused there, and names the ceiling above it', () => {
		for (const [type, ceiling] of [
			['num_anagrams', 13],
			['probability_order', 40_000],
			['point_value', 150],
			['limit_by_probability_order', 280_000]
		] as const) {
			const def = filterDef(type);
			const r = defaultRange(def, ctx);
			expect(r.max).toBe(ceiling);
			expect(rangeError(def, r.min, r.max, ctx)).toMatch(/narrows nothing/);
			expect(rangeError(def, r.min, r.max - 1, ctx)).toBeNull();
			expect(rangeError(def, r.min, ceiling + 1, ctx)).toContain(String(ceiling));
		}
	});

	it('uses the leave figures on a Leave Value cascade', () => {
		const lv: CeilingContext = { ...ctx, quizType: 'leave_value' };
		expect(ceilingOf(filterDef('length'), lv)).toBe(6);
		expect(ceilingOf(filterDef('num_anagrams'), lv)).toBe(9);
		expect(ceilingOf(filterDef('point_value'), lv)).toBe(60);
		expect(ceilingOf(filterDef('limit_by_probability_order'), lv)).toBe(900_000);
		expect(rangeError(filterDef('length'), 1, 15, lv)).toContain('6');
	});
});
