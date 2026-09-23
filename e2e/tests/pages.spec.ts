import { test, expect } from '@playwright/test';

// PLAN.md § Cascades page, § Trash page: a cascade is listed with its
// options, ladder and offline badge; Move to Trash takes it to the Trash
// grouped under its name; Restore brings it back.
test('the Cascades page lists a cascade, the Trash takes it and gives it back', async ({ page }) => {
	const name = `Eights ${Date.now()}`;
	await page.goto('/login');
	await page.getByLabel('Username').fill('dev');
	await page.getByLabel('Password').fill('correct-tile-rack-bingo');
	await page.getByRole('button', { name: 'Log in' }).click();
	await expect(page).toHaveURL(/\/cascades$/);
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill('8');
	await page.getByLabel('Maximum').first().fill('8');
	await page.getByLabel('Cascade name').fill(name);
	await page.getByRole('button', { name: 'Create Cascade' }).click();
	await expect(page).toHaveURL(/\/cascades\/[0-9a-f-]{36}$/);
	const created = new URL(page.url()).pathname;
	await page.goto('/cascades');
	const row = page.getByRole('listitem').filter({ hasText: name });
	await expect(row).toBeVisible();
	await expect(row.getByText(/of \d+ cascades/)).toHaveCount(0);
	await expect(page.getByText(/\d+ of 100 cascades/)).toBeVisible();
	await expect(row.getByText('Available offline')).toBeVisible({ timeout: 30_000 });
	await row.getByRole('button', { name: 'Move to Trash' }).click();
	await expect(page.getByText(name)).toHaveCount(0);
	await page.goto('/trash');
	await expect(page.getByText(`${name} (cascade in the Trash)`)).toBeVisible();
	await page.getByRole('listitem').filter({ hasText: `${name} (cascade in the Trash)` }).getByRole('button', { name: 'Restore' }).click();
	await expect(page.getByText(`${name} (cascade in the Trash)`)).toHaveCount(0);
	await page.goto('/cascades');
	await expect(page.getByText(name)).toBeVisible();

	// PLAN.md § Exporting words: a questions list built on the device, and a
	// spreadsheet with definitions the cards lack, which comes from the server
	// through a token and a hidden frame; both download with the source's name.
	await page.goto(`${created}/export`);
	await page.getByLabel('The questions').check();
	const counted = await page.getByText(/^\d+ questions/).textContent();
	const n = Number(counted!.split(' ')[0]);
	const [local] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	expect(local.suggestedFilename()).toBe(`${name}.txt`);
	const body = await (await local.createReadStream()).toArray();
	expect(Buffer.concat(body).toString('utf8').split('\n').filter(Boolean)).toHaveLength(n);
	await page.getByLabel('Spreadsheet (.csv)').check();
	await page.getByLabel('definition').check();
	const [remote] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Export', exact: true }).click()]);
	expect(remote.suggestedFilename()).toBe(`${name}.csv`);
	const csv = Buffer.concat(await (await remote.createReadStream()).toArray()).toString('utf8');
	expect(csv.startsWith('question,answer,definition,grade\r\n')).toBe(true);
	expect(csv.split('\r\n').filter(Boolean)).toHaveLength(n + 1);
});
