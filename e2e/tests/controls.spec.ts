import { test, expect, type Page } from '@playwright/test';
import { createCascade, login, newUser, playerPref, press } from './support';

// PLAN.md § End-to-end tests → Desktop controls.

const area = (page: Page) => page.getByRole('application', { name: 'Quiz area' });

async function at(page: Page, n: number) {
	await expect(page.getByText(new RegExp(`^${n} / 18$`))).toBeVisible({ timeout: 20_000 });
}

type Action = 'Show / Next' | 'Toggle grade' | 'Previous';

/** One action's section of the Controls card: its label, its bindings and its Add binding button. */
const section = (page: Page, action: Action) => page.locator('#controls').getByText(action, { exact: true }).locator('xpath=..');

/** Adds a binding to `action` with `stroke`. */
async function bindOn(page: Page, action: Action, stroke: (box: ReturnType<Page['getByText']>) => Promise<void>) {
	await section(page, action).getByRole('button', { name: 'Add binding' }).click();
	const box = page.getByText('Press a key, click or scroll here.');
	await expect(box).toBeVisible();
	await stroke(box);
}

test('desktop controls', async ({ page, browser, baseURL }) => {
	test.setTimeout(300_000);
	const { username } = await newUser(page, 'ctl');
	const player = await createCascade(page, { name: 'Controls' });
	await at(page, 1);
	await page.waitForTimeout(150);

	// Left click shows and advances, right click toggles, middle click goes back.
	await area(page).click({ position: { x: 30, y: 30 } });
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await page.waitForTimeout(150);
	await area(page).click({ button: 'right', position: { x: 30, y: 30 } });
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await page.waitForTimeout(150);
	await area(page).click({ position: { x: 30, y: 30 } });
	await at(page, 2);
	await page.waitForTimeout(150);
	await area(page).click({ button: 'middle', position: { x: 30, y: 30 } });
	await at(page, 1);
	// No context menu: the quiz area suppresses it.
	const prevented = await area(page).evaluate((el) => {
		const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
		el.dispatchEvent(ev);
		return ev.defaultPrevented;
	});
	expect(prevented).toBe(true);
	// A double click on a revealed card advances once (back on card 1, its saved grade is shown).
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await page.waitForTimeout(150);
	await area(page).dblclick({ position: { x: 30, y: 30 } });
	await at(page, 2);
	await expect(page.getByText('✓ Correct')).toHaveCount(0);
	// Clicks on the side rails do nothing to the quiz.
	await page.getByText(/^clear at \d+%$/).click();
	await page.getByRole('paragraph').filter({ hasText: /^attempt \d+$/ }).click({ button: 'right' });
	await at(page, 2);
	await expect(page.getByText('✓ Correct')).toHaveCount(0);

	// Rebind Toggle grade to the wheel and to Shift+T on the Account page: right
	// click makes way, since an action holds at most three bindings, and X stays
	// bound for the typed-mode check below.
	await page.goto('/account');
	const toggle = section(page, 'Toggle grade');
	await toggle.getByRole('listitem').filter({ hasText: /^Right click/ }).getByRole('button', { name: 'Remove binding' }).click();
	await expect(toggle.getByRole('listitem').filter({ hasText: /^Right click/ })).toHaveCount(0);
	await bindOn(page, 'Toggle grade', async (box) => {
		await box.hover();
		await page.mouse.wheel(0, 120);
	});
	await expect(page.getByText('Wheel down')).toBeVisible();
	await bindOn(page, 'Toggle grade', async () => {
		await page.keyboard.press('Shift+KeyT');
	});
	await expect(page.getByText('Shift+T')).toBeVisible();
	// Escape cancels capture instead of being bound.
	await page.getByRole('button', { name: 'Add binding' }).first().click();
	await page.keyboard.press('Escape');
	await expect(page.getByText('Press a key, click or scroll here.')).toHaveCount(0);
	// A stroke of another action moves, with the notice, and the old action keeps its other binding.
	await bindOn(page, 'Previous', async () => {
		await page.keyboard.press('Space');
	});
	await expect(page.getByText(/Space moved from Show \/ Next\./)).toBeVisible();
	// A binding can't be removed from an action that has only one.
	const show = section(page, 'Show / Next');
	await expect(show.getByRole('button', { name: 'Remove binding' })).toHaveCount(1);
	await show.getByRole('button', { name: 'Remove binding' }).click();
	await expect(show.getByRole('button', { name: 'Remove binding' })).toHaveCount(1);
	// Put Space back where it was.
	await bindOn(page, 'Show / Next', async () => {
		await page.keyboard.press('Space');
	});
	await expect(show.getByRole('listitem').filter({ hasText: /^Space/ })).toBeVisible();

	// Both new bindings act in the player; a wheel flick acts once.
	await page.goto(player);
	await at(page, 2);
	await press(page, 'Space');
	await expect(page.getByText('✓ Correct')).toBeVisible();
	await area(page).hover();
	await page.mouse.wheel(0, 100);
	await page.mouse.wheel(0, 100);
	await page.mouse.wheel(0, 100);
	await expect(page.getByText('✗ Missed')).toBeVisible();
	await page.waitForTimeout(200);
	await page.keyboard.press('Shift+KeyT');
	await expect(page.getByText('✓ Correct')).toBeVisible();

	// The change syncs to a second browser.
	await page.goto('/cascades');
	await expect(page.getByText('Synced')).toBeVisible({ timeout: 30_000 });
	const other = await browser.newContext({ baseURL });
	const p2 = await other.newPage();
	await login(p2, username);
	await p2.goto('/account');
	await expect(p2.getByText('Shift+T')).toBeVisible({ timeout: 30_000 });
	await expect(p2.getByText('Wheel down')).toBeVisible();
	await other.close();

	// Typed mode: bound letters, Space and Backspace type; Ctrl+Backspace, bound, still goes back.
	await page.goto('/account');
	await bindOn(page, 'Previous', async () => {
		await page.keyboard.press('Control+Backspace');
	});
	await expect(section(page, 'Previous').getByRole('listitem').filter({ hasText: /^Ctrl\+Backspace/ })).toBeVisible();
	await page.goto(player);
	await at(page, 2);
	await playerPref(page, 'Anagram answer mode', 'typed');
	const input = page.getByLabel('Type an answer');
	await input.focus();
	await page.keyboard.press('KeyX');
	await page.keyboard.press('Space');
	await page.keyboard.press('KeyA');
	await page.keyboard.press('Backspace');
	await expect(input).toHaveValue('x ');
	await expect(page.getByText('Marked missed')).toHaveCount(0);
	await page.keyboard.press('Control+Backspace');
	await at(page, 1);
	// Middle click still goes back with the input focused.
	await press(page, 'Space');
	await press(page, 'Space');
	await at(page, 2);
	await page.getByLabel('Type an answer').focus();
	await area(page).click({ button: 'middle', position: { x: 30, y: 30 } });
	await at(page, 1);
});
