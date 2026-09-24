import { test, expect, devices, type Page } from '@playwright/test';
import { createCascade, login, newUser } from './support';

// PLAN.md § End-to-end tests → Touch zones (mobile viewport emulation,
// portrait and landscape): each zone fires its action, a drag in the Show /
// Next zone scrolls without advancing, and a quick double tap advances once.

const header = (page: Page) => page.getByRole('banner');

async function at(page: Page, n: number) {
	await expect(header(page).getByText(new RegExp(`L1 · ${n}/18`))).toBeVisible({ timeout: 20_000 });
}

async function drag(page: Page, x: number, y0: number, y1: number) {
	const cdp = await page.context().newCDPSession(page);
	await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y: y0 }] });
	for (let i = 1; i <= 5; i++) {
		await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y: y0 + ((y1 - y0) * i) / 5 }] });
	}
	await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
}

for (const orientation of ['portrait', 'landscape'] as const) {
	test(`touch zones, ${orientation}`, async ({ browser, baseURL }) => {
		test.setTimeout(180_000);
		const phone = devices['Pixel 5'];
		const viewport = orientation === 'portrait' ? phone.viewport : { width: phone.viewport.height, height: phone.viewport.width };
		const setup = await browser.newContext({ baseURL });
		const sp = await setup.newPage();
		const { username } = await newUser(sp, `touch${orientation[0]}`);
		const player = await createCascade(sp, { name: 'Touch' });
		await setup.close();
		const ctx = await browser.newContext({ ...phone, viewport, baseURL });
		const page = await ctx.newPage();
		await login(page, username);
		await page.goto(player);
		await at(page, 1);
		const show = page.locator('[role="button"][tabindex="0"]').first();
		// Show / Next reveals, Toggle flips, Show / Next advances, Previous goes back.
		await show.tap();
		await expect(page.getByText('✓ Correct')).toBeVisible();
		await page.waitForTimeout(150);
		await page.getByRole('button', { name: /Toggle/ }).tap();
		await expect(page.getByText('✗ Missed')).toBeVisible();
		await page.waitForTimeout(150);
		await show.tap();
		await at(page, 2);
		await page.waitForTimeout(150);
		await page.getByRole('button', { name: /Previous/ }).tap();
		await at(page, 1);
		// A drag in the Show / Next zone scrolls and does not act.
		const box = (await show.boundingBox())!;
		await drag(page, box.x + box.width / 2, box.y + box.height * 0.7, box.y + box.height * 0.2);
		await page.waitForTimeout(300);
		await at(page, 1);
		// A quick double tap on a revealed card advances once (back on card 1, its saved grade is shown).
		await expect(page.getByText(/✓ Correct|✗ Missed/)).toBeVisible();
		await page.waitForTimeout(150);
		await show.tap();
		await show.tap();
		await at(page, 2);
		await expect(page.getByText(/✓ Correct|✗ Missed/)).toHaveCount(0);
		await ctx.close();
	});
}
