import { test, expect, type Page } from '@playwright/test';

// PLAN.md § Taking a quiz, § Saving progress, § Offline and Sync: study a new
// cascade with the default keys, go offline, keep studying, reload with no
// connection and resume at the current card.

/** A key press at a person's pace: the same action twice within 120 ms counts once. */
async function press(page: Page, key: string) {
	await page.waitForTimeout(150);
	await page.keyboard.press(key);
}

test('the player studies with the default keys, offline too', async ({ page, context }) => {
	await page.goto('/login');
	await page.getByLabel('Username').fill('dev');
	await page.getByLabel('Password').fill('correct-tile-rack-bingo');
	await page.getByRole('button', { name: 'Log in' }).click();
	await expect(page).toHaveURL(/\/cascades$/);
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill('7');
	await page.getByLabel('Maximum').first().fill('7');
	await expect(page.getByText('18 questions')).toBeVisible({ timeout: 15_000 });
	await page.getByRole('button', { name: 'Create Cascade' }).click();
	await expect(page).toHaveURL(/\/cascades\/[0-9a-f-]{36}$/);
	const url = page.url();

	// The first card, then its answer, with the default grade.
	await expect(page.getByText('1 / 18')).toBeVisible({ timeout: 20_000 });
	await press(page, 'Space');
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await press(page, 'KeyX');
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await press(page, 'Space');
	await expect(page.getByText('2 / 18')).toBeVisible();
	await expect(page.getByText('0 ✓ 1 ✗')).toBeVisible();

	// Offline: studying continues, and a reload resumes at the current card.
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
	await context.setOffline(true);
	await press(page, 'Space');
	await press(page, 'Space');
	await expect(page.getByText('3 / 18')).toBeVisible();
	await page.goto(url);
	await expect(page.getByText('3 / 18')).toBeVisible({ timeout: 20_000 });
	await expect(page.getByText('1 ✓ 1 ✗')).toBeVisible();
	await context.setOffline(false);
	// Back online, the outbox drains.
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 40_000 });
});
