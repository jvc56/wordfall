import { test, expect } from '@playwright/test';
import { createCascade, newUser, playLevel } from './support';

// PLAN.md § End-to-end tests → The journeys: "Register, confirm, log in, create
// an anagram cascade with an 80% threshold, and see every cascade rule applied
// as expected".
test('every cascade rule, from a new account', async ({ page }) => {
	test.setTimeout(180_000);
	await newUser(page, 'rules');
	await createCascade(page, { name: 'Rules', threshold: 80 });
	// Level 1 (18 questions) below the threshold: down to Level 2 with the 5 misses.
	expect(await playLevel(page, (i) => i < 5)).toBe('Level 1: 72%. Reshuffled, and its 5 missed questions are now Level 2.');
	// Level 2 cleared with a miss: replaced at Level 2.
	expect(await playLevel(page, (i) => i === 0)).toBe('Level 2 cleared with 80%. Its 1 missed questions are now Level 2.');
	// Cleared with no misses: back up to Level 1's reshuffled quiz.
	expect(await playLevel(page, () => false)).toBe('Level 2 cleared with 100%. Back to Level 1.');
	// Level 1 with no misses: the completion screen.
	expect(await playLevel(page, () => false)).toBe('Level 1: 100%. Cascade complete after 2 levels and 4 attempts.');
	await expect(page.getByRole('dialog', { name: 'Cascade complete' })).toBeVisible();
	await page.getByRole('button', { name: 'Keep studying' }).click();
	await expect(page.getByText('Level 1 of 1')).toBeVisible();
	await expect(page.getByText('1 / 18')).toBeVisible();
	await expect(page.getByRole('paragraph').filter({ hasText: /^attempt 3$/ })).toBeVisible();
});
