// The scale suites' bulk setup must leave exactly what the app would: here
// `ScaleDev.queueGrades` (one transaction) is held to `gradeAll` (one
// `applyLocally` per card) on a small cascade, so the budgets that start from
// a graded quiz start from the state a person grading it would leave.
import 'fake-indexeddb/auto';
import { describe, expect, it } from 'vitest';
import { storeCreated } from '$lib/local/created';
import { getMeta } from '$lib/local/meta';
import { FakeServer } from './fake-server';
import { ids, ScaleDev } from './scale';

const COUNT = 23;
const missed = (p: number) => p % 3 === 1;

async function graded(how: 'one by one' | 'in bulk', user: string) {
	const server = new FakeServer();
	const { cascade, source } = ids(1);
	const created = server.create({ id: cascade, source_id: source, count: COUNT });
	const d = await ScaleDev.make(server, user);
	await storeCreated(d.db, created);
	if (how === 'one by one') await d.gradeAll(source, missed);
	else await d.queueGrades(source, missed);
	const rows = (await d.db.getAll('overlay_quiz_questions')).map(({ graded_at, ...r }) => ({ ...r, graded: graded_at !== null }));
	const { last_activity_at: qa, ...quiz } = (await d.db.get('overlay_quizzes', source))!;
	const { last_activity_at: ca, ...casc } = (await d.db.get('overlay_cascades', cascade))!;
	const outbox = (await d.db.getAll('outbox')).map(({ op: { id, at, ...op }, ...e }) => ({ ...e, op, id: typeof id, at: typeof at }));
	const device = await getMeta(d.db, 'device');
	return { rows, quiz, casc, outbox, next: device.next_device_seq, stamped: [qa, ca].every((t) => typeof t === 'string') };
}

describe('the scale suites’ bulk grading', () => {
	it('leaves what grading each card with applyLocally leaves', async () => {
		const one = await graded('one by one', '55555555-5555-4555-8555-555555555555');
		const bulk = await graded('in bulk', '66666666-6666-4666-8666-666666666666');
		expect(bulk.rows).toEqual(one.rows);
		expect(bulk.quiz).toEqual(one.quiz);
		expect(bulk.casc).toEqual(one.casc);
		expect(bulk.outbox).toEqual(one.outbox);
		expect(bulk.next).toBe(one.next);
		expect(bulk.stamped && one.stamped).toBe(true);
		expect(one.outbox).toHaveLength(COUNT);
	});
});
