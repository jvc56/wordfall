// PLAN.md § Unit tests → Property tests: the seeded random sequences, shared
// with the Rust tests, run through the TypeScript rules with the cascade
// stack's invariants checked after every operation.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { Rejected, type Progression } from './rules';
import { SimCascade, type Op } from './sim';

type Json = Record<string, unknown>;
const random = JSON.parse(
	readFileSync(new URL('../../../../contract-fixtures/cascade/random.json', import.meta.url), 'utf8')
) as { vectors: Json[] };

function invariants(c: SimCascade) {
	if (c.purged) return;
	const levels = [...c.quizzes.values()].filter((q) => q.state.active).map((q) => q.state.level).sort((a, b) => a - b);
	expect(levels).toEqual(Array.from({ length: c.state.depth }, (_, i) => i + 1));
	expect(c.state.peak_depth).toBeGreaterThanOrEqual(c.state.depth);
	const source = [...c.quizzes.values()].find((q) => q.state.origin === 'source')!;
	expect(source.state.active && source.state.level === 1).toBe(true);
	const runs = new Set<string>();
	for (const q of c.quizzes.values()) {
		const s = q.state;
		expect(q.questions.every((i) => i < source.state.question_count)).toBe(true);
		expect(s.run_start <= s.cursor && s.cursor < s.question_count).toBe(true);
		if (s.segment_chain) expect([s.opts.segment_size, s.opts.progression]).toEqual([0, 'drill']);
		if (s.origin === 'segment') {
			const key = `${q.origin_quiz_id}/${q.origin_attempt}/${q.origin_segment_end}`;
			expect(runs.has(key)).toBe(false);
			runs.add(key);
		}
	}
}

describe('the shared random sequences', () => {
	for (const v of random.vectors) {
		it(v.name as string, () => {
			const s = v.setup as Json;
			const c = new SimCascade(
				s.clear_threshold as number,
				{
					segment_size: s.segment_size as number,
					progression: s.progression as Progression,
					require_alphabetical: s.require_alphabetical as boolean
				},
				s.question_count as number,
				s.source_quiz_id as string,
				BigInt(s.source_seed as string)
			);
			(v.steps as Json[]).forEach((step, i) => {
				const o = { ...(step.op as Json) };
				for (const k of ['attempt_seed', 'shuffle_seed']) if (k in o) o[k] = BigInt(o[k] as string);
				const want = step.result as Json;
				try {
					const a = c.apply(o as unknown as Op);
					expect(want.status, `${v.name} step ${i}`).toBe('applied');
					expect(a.outcome, `${v.name} step ${i}`).toBe(want.outcome);
					expect(a.new_quiz_questions_hash?.toString(), `${v.name} step ${i}`).toBe(want.new_quiz_questions_hash);
				} catch (e) {
					if (!(e instanceof Rejected)) throw e;
					expect(e.reason, `${v.name} step ${i}`).toBe(want.reason);
				}
				invariants(c);
			});
		});
	}
});
