import { test, expect, type Page } from '@playwright/test';
import { AEINRST, AEINRST_WORDS, apiCascade, newUser, playerPref, press } from './support';

// PLAN.md § End-to-end tests: typed mode, and alphabetical order.

async function open(page: Page, player: string) {
	await page.goto(player);
	await expect(page.getByText('1 / 1')).toBeVisible({ timeout: 30_000 });
}

async function type(page: Page, text: string) {
	const input = page.getByLabel('Type an answer');
	await input.fill(text);
	await input.press('Enter');
}

/**
 * Moves on from the revealed card of a one-question cascade. A correct card
 * finishes the Source quiz with no misses, so "Cascade complete" follows and is
 * dismissed; a missed one reshuffles it.
 */
async function next(page: Page, completes: boolean) {
	const input = page.getByLabel('Type an answer');
	if (await input.count()) await input.blur();
	await press(page, 'Space');
	const keep = page.getByRole('button', { name: 'Keep studying' });
	if (completes) {
		await keep.click({ timeout: 15_000 });
		await expect(keep).toHaveCount(0);
	}
	await expect(page.getByText(/✓ Correct|✗ Missed/)).toHaveCount(0);
	await expect(page.getByText('1 / 1')).toBeVisible();
}

test('typed mode', async ({ page }) => {
	test.setTimeout(240_000);
	await newUser(page, 'typed');
	const player = await apiCascade(page, { name: 'AEINRST', filters: AEINRST });
	await open(page, player);
	await playerPref(page, 'Anagram answer mode', 'typed');
	await expect(page.getByText('0 of 9 found')).toBeVisible();

	// A wrong entry, then Enter on an empty input reveals: missed.
	await type(page, 'EINARST');
	await expect(page.getByText('EINARST')).toBeVisible();
	await type(page, '');
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await next(page, false);

	// A repeated entry is ignored; two words on one line stay in the input.
	await type(page, 'retains');
	await type(page, 'RETAINS');
	await expect(page.getByText('Already entered')).toBeVisible();
	await page.getByLabel('Type an answer').fill('RETAINS NASTIER');
	await page.getByLabel('Type an answer').press('Enter');
	await expect(page.getByText('Type one word at a time')).toBeVisible();
	await expect(page.getByLabel('Type an answer')).toHaveValue('RETAINS NASTIER');
	// Every anagram found: correct.
	for (const w of AEINRST_WORDS.filter((w) => w !== 'RETAINS')) await type(page, w);
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await next(page, true);

	// Before the reveal, a right click still toggles; a left click only focuses the input.
	await page.getByLabel('Type an answer').blur();
	const area = page.getByRole('application', { name: 'Quiz area' });
	await area.click({ button: 'right', position: { x: 20, y: 20 } });
	await expect(page.getByText('Marked missed')).toBeVisible();
	await area.click({ position: { x: 20, y: 20 } });
	await expect(page.getByLabel('Type an answer')).toBeFocused();
	await expect(page.getByText('✓ Correct')).toHaveCount(0);
	// Switching modes mid-card clears the entries.
	await type(page, 'RETAINS');
	await expect(page.getByText('1 of 9 found')).toBeVisible();
	await playerPref(page, 'Anagram answer mode', 'flashcard');
	await playerPref(page, 'Anagram answer mode', 'typed');
	await expect(page.getByText('0 of 9 found')).toBeVisible();
});

test('alphabetical order in typed mode', async ({ page }) => {
	test.setTimeout(180_000);
	await newUser(page, 'alpha');
	const ordered = await apiCascade(page, { name: 'Ordered', filters: AEINRST, alphabetical: true });
	await open(page, ordered);
	await playerPref(page, 'Anagram answer mode', 'typed');
	// In order: correct.
	for (const w of AEINRST_WORDS) await type(page, w);
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await next(page, true);
	// Out of order: marked, found, and the card missed; a later in-order answer is accepted.
	await type(page, 'NASTIER');
	await type(page, 'ANTSIER');
	await expect(page.getByText('Out of order')).toBeVisible();
	await expect(page.getByText('2 of 9 found')).toBeVisible();
	await type(page, 'RATINES');
	await expect(page.getByText('3 of 9 found')).toBeVisible();
	for (const w of ['ANESTRI', 'RETAINS', 'RETINAS', 'RETSINA', 'STAINER', 'STEARIN']) await type(page, w);
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await next(page, false);
	// With the option off, the same sequence grades correct.
	const plain = await apiCascade(page, { name: 'Plain', filters: AEINRST });
	await open(page, plain);
	for (const w of ['NASTIER', 'ANTSIER', 'RATINES', 'ANESTRI', 'RETAINS', 'RETINAS', 'RETSINA', 'STAINER', 'STEARIN']) await type(page, w);
	await expect(page.getByText('✓ Correct')).toBeVisible();
});
