import { test, expect, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { PROJECT } from '../stack';
import { createCascade, idb, login, newUser, outboxCount, playLevel, playerPref, press } from './support';

// PLAN.md § End-to-end tests: answers gone with questions kept, a download
// stopped part way, Trash and export offline, and export through the server.

async function ready(page: Page) {
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
}

/** Waits for the named cascade's own "Available offline" badge (another cascade's would not do). */
async function availableOffline(page: Page, name: string) {
	await page.goto('/cascades');
	const entry = page.getByRole('listitem').filter({ has: page.getByRole('link', { name, exact: true }) });
	await expect(entry.getByText('Available offline')).toBeVisible({ timeout: 60_000 });
	await ready(page);
}

/** Stands in for eviction: the cascade's answers go, its keys and rows stay. */
async function evictAnswers(page: Page) {
	await idb(page, `const tx = db.transaction('cards', 'readwrite'); tx.objectStore('cards').clear(); await new Promise((r) => (tx.oncomplete = r));`);
}

test('answers gone, questions kept; a download stopped part way', async ({ page, context, browser, baseURL }) => {
	test.setTimeout(240_000);
	const { username } = await newUser(page, 'gone');
	const player = await createCascade(page, { name: 'Evicted' });
	await availableOffline(page, 'Evicted');
	await page.goto(player);
	await playerPref(page, 'Anagram answer mode', 'typed');
	await evictAnswers(page);
	await context.setOffline(true);
	await page.goto(player);
	// The alphagram, and the flashcard fallback rather than a typed input.
	await expect(page.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	await expect(page.getByLabel('Type an answer')).toHaveCount(0);
	await press(page, 'Space');
	await expect(page.getByText('Answer needs a connection.')).toBeVisible();
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await press(page, 'Space');
	await context.setOffline(false);
	await page.goto('/cascades');
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 60_000 });
	// The grade reached the server as Correct.
	const fresh = await browser.newContext({ baseURL });
	const p2 = await fresh.newPage();
	await login(p2, username);
	await p2.goto(player);
	await expect(p2.getByText('1 ✓ 0 ✗')).toBeVisible({ timeout: 60_000 });
	await fresh.close();

	// A download stopped part way: only the first two cards' keys arrived.
	const partial = await createCascade(page, { name: 'Partial' });
	await availableOffline(page, 'Partial');
	await context.setOffline(true);
	await idb(
		page,
		`const quizzes = await req(db.transaction('quizzes').objectStore('quizzes').index('cascade_id').getAll(arg));
		 const q = quizzes.find((x) => x.origin === 'source');
		 const rows = await req(db.transaction('quiz_questions').objectStore('quiz_questions').index('quiz_id').getAll(q.id));
		 const keep = new Set(rows.filter((r) => r.position < 2).map((r) => r.question_idx));
		 const tx = db.transaction(['questions', 'cards'], 'readwrite');
		 for (const r of rows) if (!keep.has(r.question_idx)) { tx.objectStore('questions').delete([arg, r.question_idx]); tx.objectStore('cards').delete([arg, r.question_idx]); }
		 await new Promise((res) => (tx.oncomplete = res));`,
		partial.split('/').pop()
	);
	await page.goto(partial);
	await expect(page.getByText('1 / 18')).toBeVisible({ timeout: 30_000 });
	// Flashcards again: in typed mode Space would type into the answer rather than move on.
	await playerPref(page, 'Anagram answer mode', 'flashcard');
	for (let i = 0; i < 2; i++) {
		await press(page, 'Space');
		await press(page, 'Space');
	}
	await expect(page.getByText('This question needs a connection.')).toBeVisible();
	const before = await outboxCount(page, 'grade');
	await press(page, 'Space');
	await expect(page.getByText('4 / 18')).toBeVisible();
	expect(await outboxCount(page, 'grade')).toBe(before);
	for (let i = 4; i < 18; i++) await press(page, 'Space');
	await expect(page.getByText('18 / 18')).toBeVisible();
	await press(page, 'Space');
	await expect(page.getByText('Some questions in this run still need a connection (16).')).toBeVisible();
	await expect(page.getByText('18 / 18')).toBeVisible();
	await context.setOffline(false);
});

test('trash: export a cleared quiz’s missed words offline, see purge dates, restore it playable at once', async ({ page, context }) => {
	test.setTimeout(240_000);
	await newUser(page, 'trashj');
	const player = await createCascade(page, { name: 'Trash journey' });
	expect(await playLevel(page, (i) => i < 5)).toMatch(/^Level 1/);
	expect(await playLevel(page, (i) => i === 0)).toBe('Level 2 cleared with 80%. Its 1 missed questions are now Level 2.');
	await availableOffline(page, 'Trash journey');
	await context.setOffline(true);
	await page.goto('/trash');
	await expect(page.getByText(/^purges \d/).first()).toBeVisible();
	// Export its missed words from the Trash, offline, with no fetch.
	const requests: string[] = [];
	page.on('request', (r) => {
		if (r.url().includes('/api/')) requests.push(r.url());
	});
	await page.getByRole('link', { name: 'Export…' }).first().click();
	await page.getByLabel('Missed').check();
	await page.getByLabel('The questions').check();
	const [dl] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	expect(dl.suggestedFilename()).toBe('Trash journey - L2 missed.txt');
	expect(readFileSync((await dl.path())!, 'utf8').split('\n').filter(Boolean)).toHaveLength(1);
	expect(requests.filter((u) => !u.endsWith('/api/sync'))).toEqual([]);
	// Online, restore it: the new deepest level, playable at once.
	await context.setOffline(false);
	await page.goto('/trash');
	await page.getByRole('button', { name: 'Restore' }).first().click();
	await expect(page.getByText('The Trash is empty.')).toBeVisible();
	await page.goto(player);
	await expect(page.getByText('Level 3 of 3')).toBeVisible({ timeout: 30_000 });
	await expect(page.getByText('1 / 5')).toBeVisible();
});

test('export: counts, offline files, the definitions fallback, and one file through the server', async ({ page, context }) => {
	test.setTimeout(240_000);
	await newUser(page, 'exp');
	const player = await createCascade(page, { name: 'Export journey' });
	expect(await playLevel(page, (i) => i < 3)).toMatch(/^Level 1/);
	// Level 2 part played: its first card missed, its second correct. A cascade
	// export's grades are the active quizzes' current attempts (§ Exporting words),
	// and the finished Level 1 attempt is behind the Source quiz's reset.
	await expect(page.getByText('1 / 3')).toBeVisible();
	await press(page, 'Space');
	await press(page, 'KeyX');
	await press(page, 'Space');
	await expect(page.getByText('2 / 3')).toBeVisible();
	await press(page, 'Space');
	await press(page, 'Space');
	await expect(page.getByText('3 / 3')).toBeVisible();
	await availableOffline(page, 'Export journey');
	const exportPage = `${player}/export`;
	// With the cards complete, the question count and the entry count.
	await page.goto(exportPage);
	await expect(page.getByText(/^\d+ questions · \d+ words$/)).toBeVisible();
	// The cascade export offers no alphabetical toggle.
	await expect(page.getByLabel(/Alphabetical instead/)).toHaveCount(0);
	await context.setOffline(true);
	// Offline: a level's missed words as a word list …
	await page.getByLabel(/Level 2/).check();
	const [words] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	expect(words.suggestedFilename()).toBe('Export journey - L2.txt');
	expect(readFileSync((await words.path())!, 'utf8').split('\n').filter(Boolean).length).toBeGreaterThanOrEqual(3);
	// … and the whole cascade as a CSV.
	await page.getByLabel('The whole cascade').check();
	await page.getByLabel('Spreadsheet (.csv)').check();
	const [csv] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	const text = readFileSync((await csv.path())!, 'utf8');
	expect(text.startsWith('question,answer,grade\r\n')).toBe(true);
	expect(text.split('\r\n').filter(Boolean)).toHaveLength(19);
	expect(text.split('\r\n').filter((l) => l.endsWith(',missed'))).toHaveLength(1);
	expect(text.split('\r\n').filter((l) => l.endsWith(',correct'))).toHaveLength(1);
	// Definitions never downloaded, offline: the fallback notice.
	await page.getByLabel('definition').check();
	await page.getByRole('button', { name: 'Export', exact: true }).click();
	await expect(page.getByText('This export needs a connection.')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Export the questions alone' })).toBeVisible();
	// Answers evicted and offline: the question count, no entry count; the questions still export whole.
	await evictAnswers(page);
	await page.reload();
	await expect(page.getByText(/^18 questions$/)).toBeVisible();
	await page.getByLabel('Word list (.txt)').check();
	await page.getByLabel('The questions').check();
	const [qs] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	expect(readFileSync((await qs.path())!, 'utf8').split('\n').filter(Boolean)).toHaveLength(18);
	await context.setOffline(false);
	// Online, definitions from the server: a double click gives one file and the app stays.
	await page.goto(exportPage);
	await page.getByLabel('Spreadsheet (.csv)').check();
	await page.getByLabel('definition').check();
	const statuses: number[] = [];
	page.on('response', (r) => {
		if (/\/export\?/.test(r.url())) statuses.push(r.status());
	});
	const downloads: string[] = [];
	page.on('download', (d) => downloads.push(d.suggestedFilename()));
	await page.getByRole('button', { name: 'Export', exact: true }).dblclick();
	await expect.poll(() => downloads.length, { timeout: 30_000 }).toBe(1);
	await page.waitForTimeout(2000);
	expect(downloads).toEqual(['Export journey.csv']);
	await expect(page.getByRole('heading', { name: /Export Export journey/ })).toBeVisible();
	expect(statuses.filter((s) => s === 200)).toHaveLength(1);
	// The backend stops between the token request and the download: the frame
	// meets the proxy's error page, the app stays with no file, and Export can be pressed again.
	await page.route('**/api/cascades/*/export?*', async (route) => {
		execFileSync('docker', ['compose', '-p', PROJECT, 'stop', 'backend']);
		await route.continue();
	});
	try {
		const before = downloads.length;
		await page.getByRole('button', { name: 'Export', exact: true }).click();
		await page.waitForTimeout(5000);
		expect(downloads.length).toBe(before);
		await expect(page.getByRole('heading', { name: /Export Export journey/ })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Export', exact: true })).toBeEnabled();
	} finally {
		await page.unroute('**/api/cascades/*/export?*');
		execFileSync('docker', ['compose', '-p', PROJECT, 'start', 'backend']);
		await expect.poll(async () => (await page.request.get('/health').catch(() => null))?.status(), { timeout: 120_000 }).toBe(200);
	}
});
