import { test, expect } from '@playwright/test';

// PLAN.md § Deployment and Operations → Security headers: the shell loads under
// its own CSP (a missing hash would leave a blank page) with Nginx's headers.
test('the app shell boots under its CSP with the Nginx headers', async ({ page }) => {
	const errors: string[] = [];
	page.on('console', (m) => {
		if (m.type() === 'error') errors.push(m.text());
	});
	const resp = await page.goto('/');
	expect(resp?.headers()['content-security-policy']).toBe("frame-ancestors 'self'");
	expect(resp?.headers()['x-content-type-options']).toBe('nosniff');
	expect(resp?.headers()['x-frame-options']).toBeUndefined();
	await expect(page.getByRole('heading', { name: 'Wordfall' })).toBeVisible();
	expect(errors.filter((e) => /Content Security Policy/i.test(e))).toEqual([]);
});

// PLAN.md § On the device → App shell: once the service worker is active, a
// reload of an app route with no connection still loads the app, while /api
// navigations go to the network untouched.
test('the service worker serves the shell offline', async ({ page, context }) => {
	await page.goto('/');
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
	// A reload so the page is controlled.
	await page.reload();
	await expect.poll(() => page.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);
	await context.setOffline(true);
	await page.goto('/login');
	await expect(page.getByLabel('Username')).toBeVisible();
	await page.goto('/cascades/00000000-0000-4000-8000-000000000000').catch(() => undefined);
	await expect(page.locator('body')).not.toBeEmpty();
	await context.setOffline(false);
});
