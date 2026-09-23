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
