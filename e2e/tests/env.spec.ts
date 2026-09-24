import { test, expect, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { PROJECT } from '../stack';
import { createCascade, login, newUser, playLevel, press } from './support';

// PLAN.md § End-to-end tests: the journeys that need the stack configured
// through the same --env flag a developer uses. Each tag runs in its own pass
// of `make test-e2e`, with its own stack (see the Makefile).

async function ready(page: Page) {
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
}

// Session expiry while offline (SESSION_TTL_SECONDS short; PQ-016).
test('@env @ttl session expiry while offline', async ({ page, context }) => {
	test.setTimeout(180_000);
	const { username } = await newUser(page, 'ttl');
	const player = await createCascade(page, { name: 'Expiring' });
	await page.goto('/cascades');
	await expect(page.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await ready(page);
	await context.setOffline(true);
	await page.goto(player);
	await press(page, 'Space');
	await press(page, 'Space');
	await press(page, 'Space');
	await press(page, 'Space');
	await expect(page.getByText('3 / 18')).toBeVisible();
	// Past the session's life, reconnect: Log in to sync, the work kept.
	await page.waitForTimeout(Number(process.env.E2E_TTL_WAIT_MS ?? 25_000));
	await context.setOffline(false);
	await page.goto('/cascades');
	await expect(page.getByText('Log in to sync')).toBeVisible({ timeout: 60_000 });
	await login(page, username);
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	// The work reached the server: a fresh browser sees it.
	const fresh = await page.context().browser()!.newContext({ baseURL: page.url().split('/cascades')[0] });
	const p2 = await fresh.newPage();
	await login(p2, username);
	await p2.goto(player);
	await expect(p2.getByText('3 / 18')).toBeVisible({ timeout: 60_000 });
	await fresh.close();
});

// Trash and purge (TRASH_RETENTION_DAYS=0 with a short purge interval).
test('@env @purge a cleared quiz is purged and the tombstone removes it from a second browser', async ({ page, browser, baseURL }) => {
	test.setTimeout(240_000);
	const { username } = await newUser(page, 'purge');
	const player = await createCascade(page, { name: 'Purged' });
	const other = await browser.newContext({ baseURL });
	const p2 = await other.newPage();
	await login(p2, username);
	await p2.goto(player);
	await expect(p2.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	expect(await playLevel(page, (i) => i < 2)).toMatch(/^Level 1/);
	expect(await playLevel(page, () => false)).toMatch(/^Level 2 cleared/);
	await p2.goto('/trash');
	await expect(p2.getByText(/Purged/)).toBeVisible({ timeout: 60_000 });
	// The purge task takes it, and the next pull's tombstone removes it.
	await expect
		.poll(
			async () => {
				await p2.goto('/trash');
				return await p2.getByText('The Trash is empty.').isVisible();
			},
			{ timeout: 120_000, intervals: [3000] }
		)
		.toBe(true);
	await other.close();
});

// The saved-search limit (MAX_SAVED_SEARCHES_PER_USER lowered).
test('@env @limits Save Search at the limit is refused with the limit and count; an overwrite is accepted', async ({ page }) => {
	test.setTimeout(120_000);
	await newUser(page, 'limits');
	const limit = Number(process.env.E2E_SAVED_LIMIT ?? 2);
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill('7');
	await page.getByLabel('Maximum').first().fill('7');
	await expect(page.getByText(/\d+ questions/).first()).toBeVisible({ timeout: 15_000 });
	const save = async (name: string) => {
		await page.getByRole('button', { name: 'Save Search…' }).click();
		await page.getByPlaceholder('Name').fill(name);
		await page.getByRole('button', { name: 'Save', exact: true }).click();
	};
	for (let i = 0; i < limit; i++) {
		await save(`S${i}`);
		await expect(page.getByRole('dialog')).toHaveCount(0);
	}
	await save('one too many');
	await expect(page.getByText(`You have ${limit} of ${limit} saved searches, the most you can keep. Delete one first.`)).toBeVisible();
	await page.keyboard.press('Escape');
	page.on('dialog', (d) => void d.accept());
	await save('S0');
	await expect(page.getByRole('dialog')).toHaveCount(0);
});

// A cascade whose segmented attempt left 5,000 cleared chain quizzes opens /trash in budget.
test('@env @limits /trash with 5,000 cleared chain quizzes: one collapsed group, pages of 100', async ({ page }) => {
	test.setTimeout(240_000);
	const { username } = await newUser(page, 'trash5k');
	const player = await createCascade(page, { name: 'Many chains' });
	const cascade = player.split('/').pop()!;
	// The chain quizzes as a segmented attempt leaves them, written to the database.
	execFileSync('docker', ['compose', '-p', PROJECT, 'exec', '-T', 'postgres', 'psql', '-U', 'wordfall', '-d', 'wordfall', '-v', 'ON_ERROR_STOP=1', '-c', `
		WITH u AS (UPDATE users SET sync_seq = sync_seq + 1 WHERE username = '${username}' RETURNING id, sync_seq),
		     src AS (SELECT q.id, q.cascade_id FROM quizzes q WHERE q.cascade_id = '${cascade}' AND q.origin = 'source'),
		     ins AS (
		       INSERT INTO quizzes (id, cascade_id, user_id, level, origin, origin_quiz_id, origin_attempt, origin_segment_end,
		                            status, segment_chain, segment_size, progression, options_changed_at, options_seq,
		                            options_device_id, shuffle_seed, questions_hash, question_count, correct_count,
		                            created_seq, updated_seq, cleared_at, last_activity_at)
		       SELECT gen_random_uuid(), src.cascade_id, u.id, 2, 'segment', src.id, g, 5, 'cleared', true, 0, 'drill', now(),
		              u.sync_seq, gen_random_uuid(), g, 0, 1, 1, u.sync_seq, u.sync_seq, now() - interval '1 day', now()
		       FROM u, src, generate_series(1, 5000) AS g RETURNING 1)
		SELECT count(*) FROM ins;`], { encoding: 'utf8' });
	await page.goto('/cascades');
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	const t = Date.now();
	await page.goto('/trash');
	const group = page.getByRole('button', { name: /Many chains/ });
	await expect(group).toContainText('5000 entries', { timeout: 30_000 });
	expect(Date.now() - t).toBeLessThan(15_000);
	await expect(page.getByRole('button', { name: 'Restore' })).toHaveCount(0);
	await group.click();
	await expect(page.getByRole('button', { name: 'Restore' })).toHaveCount(100);
	await page.getByRole('button', { name: 'Show more' }).click();
	await expect(page.getByRole('button', { name: 'Restore' })).toHaveCount(200);
});
