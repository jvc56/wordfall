import { test, expect } from '@playwright/test';

// A smoke run of the builder against the seeded fixture catalog (PLAN.md §
// Creating a cascade): preview as filters change, then Create Cascade.
test('the builder previews and creates a cascade', async ({ page }) => {
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
	await expect(page.getByLabel('Cascade name')).toHaveValue('EN-FIX · Length 7–7');
	await page.getByRole('button', { name: 'Create Cascade' }).click();
	await expect(page).toHaveURL(/\/cascades\/[0-9a-f-]{36}$/);
});
