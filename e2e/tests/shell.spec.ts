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
	// The index.html the worker serves carries the build's <meta> policy: script-src
	// holds 'self' and a hash, never 'unsafe-inline'; an injected inline script is refused.
	const html = await page.evaluate(async () => (await (await caches.match('/index.html'))?.text()) ?? '');
	const policy = /<meta http-equiv="content-security-policy" content="([^"]+)"/i.exec(html)?.[1] ?? '';
	const scriptSrc = policy.split(';').find((d) => d.trim().startsWith('script-src')) ?? '';
	expect(scriptSrc).toContain("'self'");
	expect(scriptSrc).toMatch(/'sha256-[^']+'/);
	expect(scriptSrc).not.toContain('unsafe-inline');
	const refused = await page.evaluate(
		() =>
			new Promise<boolean>((resolve) => {
				document.addEventListener('securitypolicyviolation', () => resolve(true), { once: true });
				const s = document.createElement('script');
				s.textContent = 'window.__injected = true';
				document.body.appendChild(s);
				setTimeout(() => resolve(false), 1000);
			})
	);
	expect(refused).toBe(true);
	expect(await page.evaluate(() => (window as unknown as { __injected?: boolean }).__injected)).toBeUndefined();
	await context.setOffline(true);
	await page.goto('/login');
	await expect(page.getByLabel('Username')).toBeVisible();
	await page.goto('/cascades/00000000-0000-4000-8000-000000000000').catch(() => undefined);
	await expect(page.locator('body')).not.toBeEmpty();
	await context.setOffline(false);
});
