import { test, expect } from '@playwright/test';
import { newUser } from './support';

// PLAN.md § End-to-end tests: "Paste a 10,001-entry word list into an In Word
// List row and see the live preview pause behind a Preview button; change the
// lexicon and see an In Lexicon row on another distribution flagged."
test('a long word list pauses the live preview; a lexicon change flags an In Lexicon row', async ({ page }) => {
	test.setTimeout(180_000);
	await newUser(page, 'lists');
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').first().selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill('2');
	await page.getByLabel('Maximum').first().fill('15');
	await expect(page.getByText(/\d+ questions/).first()).toBeVisible({ timeout: 15_000 });
	// An In Word List row with 10,001 entries.
	await page.getByRole('button', { name: 'Add row' }).first().click();
	await page.getByLabel('Filter type').nth(1).selectOption({ label: 'In Word List' });
	const words: string[] = [];
	const letters = 'ABCDEFGHILMNOPRSTU';
	for (let i = 0; words.length < 10_001; i++) {
		let n = i;
		let w = '';
		for (let k = 0; k < 7; k++) {
			w += letters[n % letters.length];
			n = Math.floor(n / letters.length);
		}
		words.push(w);
	}
	// Pasted, as the plan says: Playwright's fill types the text in, which a
	// 10,001-line textarea takes minutes to lay out.
	await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
	await page.evaluate((t) => navigator.clipboard.writeText(t), words.join('\n'));
	await page.getByPlaceholder('One entry per line').focus();
	await page.keyboard.press('ControlOrMeta+V');
	await page.getByRole('button', { name: 'Use this list' }).click();
	await expect(page.getByText('10,001 entries.')).toBeVisible({ timeout: 30_000 });
	await expect(page.getByText('A word list this long is previewed only when you ask.')).toBeVisible();
	await page.getByRole('button', { name: 'Preview', exact: true }).click();
	await expect(page.getByText(/^\d+ questions/).first()).toBeVisible({ timeout: 30_000 });
	// An In Lexicon row naming a lexicon on the English distribution …
	await page.getByRole('button', { name: 'Add row' }).first().click();
	await page.getByLabel('Filter type').nth(2).selectOption({ label: 'In Lexicon' });
	await page.getByLabel('Lexicon').nth(1).selectOption('EN-FIX-OLD');
	// … is flagged once the cascade's lexicon is on another distribution.
	await page.getByLabel('Lexicon').first().selectOption('CA-FIX');
	await expect(page.locator('p.text-destructive').filter({ hasText: /distribution/ })).toBeVisible({ timeout: 10_000 });
});
