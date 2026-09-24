// Shared steps for the journeys (PLAN.md § End-to-end tests): each spec
// registers its own user through the real endpoints, reading the
// confirmation code from the backend's console mail log exactly as a
// developer confirms an account locally.
import { execFileSync } from 'node:child_process';
import { expect, type Page } from '@playwright/test';
import { PROJECT } from '../stack';

export const PASSWORD = 'correct-tile-rack-bingo';

function code(email: string): string {
	const deadline = Date.now() + 30_000;
	const pattern = new RegExp(`"mail_to":"${email.replace(/[.+]/g, '\\$&')}".*?"confirmation_code":"([^"]+)"`, 'g');
	for (;;) {
		const logs = execFileSync('docker', ['compose', '-p', PROJECT, 'logs', '--no-color', 'backend'], { encoding: 'utf8' });
		const found = [...logs.matchAll(pattern)].map((m) => m[1]);
		if (found.length) return found[found.length - 1];
		if (Date.now() > deadline) throw new Error(`no confirmation code for ${email}`);
		execFileSync('sleep', ['0.5']);
	}
}

/** A new confirmed user, logged in on `page`. */
export async function newUser(page: Page, prefix = 'e2e'): Promise<{ username: string; email: string }> {
	const tag = `${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
	const username = `${prefix}${tag}`.slice(0, 30);
	const email = `${username}@example.test`;
	const reg = await page.request.post('/api/auth/register', { data: { username, email, password: PASSWORD } });
	expect([200, 201, 202]).toContain(reg.status());
	const confirm = await page.request.post('/api/auth/confirm-email', { data: { code: code(email) } });
	expect([200, 204]).toContain(confirm.status());
	await login(page, username);
	return { username, email };
}

export async function login(page: Page, username: string) {
	await page.goto('/login');
	await page.getByLabel('Username').fill(username);
	await page.getByLabel('Password').fill(PASSWORD);
	await page.getByRole('button', { name: 'Log in' }).click();
	await expect(page).toHaveURL(/\/cascades$/);
}

/** A cascade through the builder; returns its player path. */
export async function createCascade(
	page: Page,
	o: { name: string; min?: number; max?: number; segment?: number; threshold?: number } = { name: 'x' }
): Promise<string> {
	await page.goto('/cascades/new');
	await page.getByLabel('Lexicon').selectOption('EN-FIX');
	await page.getByLabel('Minimum').first().fill(String(o.min ?? 7));
	await page.getByLabel('Maximum').first().fill(String(o.max ?? 7));
	await expect(page.getByText(/\d+ questions/).first()).toBeVisible({ timeout: 15_000 });
	if (o.segment !== undefined) {
		await page.getByLabel('Study in segments').check();
		await page.getByLabel('Segment size').fill(String(o.segment));
	}
	if (o.threshold !== undefined) await page.getByLabel('Clear threshold (%)').fill(String(o.threshold));
	await page.getByLabel('Cascade name').fill(o.name);
	await page.getByRole('button', { name: 'Create Cascade' }).click();
	await expect(page).toHaveURL(/\/cascades\/[0-9a-f-]{36}$/);
	return new URL(page.url()).pathname;
}

/**
 * A cascade created through the API with the session the page holds; the
 * device learns of it at its next sync, as it would of one made elsewhere.
 */
export async function apiCascade(
	page: Page,
	o: { name: string; quiz_type?: string; lexicon?: string; filters: unknown; segment_size?: number; progression?: string; alphabetical?: boolean }
): Promise<string> {
	const csrf = (await page.context().cookies()).find((c) => c.name === 'wordfall_csrf')!.value;
	const me = await (await page.request.get('/api/auth/me')).json();
	const id = crypto.randomUUID();
	const r = await page.request.post('/api/cascades', {
		headers: { 'X-CSRF-Token': csrf, 'X-Wordfall-User': me.user_id },
		data: {
			id,
			source_quiz_id: crypto.randomUUID(),
			device_id: crypto.randomUUID(),
			at: new Date().toISOString(),
			name: o.name,
			lexicon: o.lexicon ?? 'EN-FIX',
			quiz_type: o.quiz_type ?? 'anagram',
			clear_threshold: 80,
			segment_size: o.segment_size ?? 0,
			progression: o.progression ?? 'ladder',
			require_alphabetical: o.alphabetical ?? false,
			filters: o.filters
		}
	});
	expect(r.status(), await r.text()).toBe(201);
	return `/cascades/${id}`;
}

/** The one-question AEINRST cascade (nine anagrams on EN-FIX). */
// A pattern's wire form is its tiles separated by spaces, as the builder canonicalises it.
export const AEINRST = { op: 'and', children: [{ type: 'anagram_match', negated: false, pattern: 'A E I N R S T' }] };
export const AEINRST_WORDS = ['ANESTRI', 'ANTSIER', 'NASTIER', 'RATINES', 'RETAINS', 'RETINAS', 'RETSINA', 'STAINER', 'STEARIN'];

/** Sets a preference in the player's settings menu. */
export async function playerPref(page: Page, label: string, value: string | boolean) {
	await page.getByRole('button', { name: /Preferences|Settings/ }).first().click();
	const control = page.getByRole('dialog', { name: 'Settings' }).getByLabel(label);
	if (typeof value === 'boolean') await control.setChecked(value);
	else await control.selectOption(value);
	await page.getByRole('dialog', { name: 'Settings' }).getByRole('button', { name: 'Close' }).click();
}

/**
 * Runs `fn` against the signed-in account's IndexedDB in the page. Used to
 * read what the device holds, and to stand in for eviction and for a
 * download stopped part way — states the plan's journeys start from.
 */
export async function idb<T>(page: Page, fn: string, arg?: unknown): Promise<T> {
	// A string expression goes through DevTools evaluation, which the page's CSP
	// (no 'unsafe-eval') does not govern, unlike `new Function` inside the page.
	const source = `(async (arg) => {
		const open = (name) =>
			new Promise((res, rej) => {
				const r = indexedDB.open(name);
				r.onsuccess = () => res(r.result);
				r.onerror = () => rej(r.error);
			});
		const req = (r) =>
			new Promise((res, rej) => {
				r.onsuccess = () => res(r.result);
				r.onerror = () => rej(r.error);
			});
		const unscoped = await open('wordfall');
		const pointer = await req(unscoped.transaction('pointer').objectStore('pointer').get('signed_in'));
		unscoped.close();
		const db = await open('wordfall-user-' + pointer.user_id);
		try {
			return await (async () => { ${fn} })();
		} finally {
			db.close();
		}
	})(${JSON.stringify(arg ?? null)})`;
	return page.evaluate(source) as Promise<T>;
}

/** Operations of a kind waiting in the outbox. */
export async function outboxCount(page: Page, type?: string): Promise<number> {
	return idb(page, `const all = await req(db.transaction('outbox').objectStore('outbox').getAll()); return all.filter((e) => !arg || e.op.type === arg).length;`, type);
}

/** A key press at a person's pace: the same action twice within 120 ms counts once. */
export async function press(page: Page, key: string) {
	await page.waitForTimeout(150);
	await page.keyboard.press(key);
}

/** The progress on the right rail, `37 / 250`. */
export async function progress(page: Page): Promise<[number, number]> {
	const t = await page.getByText(/^\d+ \/ \d+$/).first().textContent();
	const [a, b] = t!.split(' / ').map(Number);
	return [a, b];
}

/**
 * Plays the current level through with Space, marking a card missed with X
 * when `miss(i)` says so; returns the banner.
 */
export async function playLevel(page: Page, miss: (i: number) => boolean): Promise<string> {
	// Let the level the last finish led to render before reading it.
	await page.waitForTimeout(400);
	const [pos, count] = await progress(page);
	const banner = page.getByRole('status').filter({ hasText: /^(Level \d|Run \d)/ }).first();
	for (let i = pos - 1; i < count; i++) {
		await expect(page.getByText(new RegExp(`^${i + 1} / ${count}$`)).first()).toBeVisible({ timeout: 15_000 });
		await press(page, 'Space');
		await expect(page.getByText(/✓ Correct|✗ Missed/)).toBeVisible();
		if (miss(i - (pos - 1))) await press(page, 'KeyX');
		await press(page, 'Space');
		// Either the next card or a banner (the attempt or the run is over).
		if (i + 1 < count) await expect(banner.or(page.getByText(new RegExp(`^${i + 2} / ${count}$`))).first()).toBeVisible({ timeout: 15_000 });
		if (await banner.isVisible()) break;
	}
	await expect(banner).toBeVisible({ timeout: 15_000 });
	return (await banner.textContent())!.trim();
}
