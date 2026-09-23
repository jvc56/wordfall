// PLAN.md § Unit tests → Frontend: "The export formatters are held to one set
// of fixtures on both sides" — contract-fixtures/export/cases.json, written by
// an independent reference (contract-fixtures/tools/gen_export.py).
import { describe, expect, it } from 'vitest';
import fixtures from '../../../../contract-fixtures/export/cases.json';
import { exportFilename, formatExport, type ExportInput } from './format';

describe('the export formatter', () => {
	for (const c of fixtures.cases as unknown as (ExportInput & { case: string; expected: { filename: string; body: string } })[]) {
		it(c.case, () => {
			expect([...formatExport(c, 2)].join('')).toBe(c.expected.body);
			expect(exportFilename(c.name, c.choices)).toBe(c.expected.filename);
		});
	}
});
