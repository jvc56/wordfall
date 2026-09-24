<script lang="ts">
	// Offline storage (PLAN.md § On the device → Downloads, the budget, the
	// quota path): the device's rows and keys against ROW_STORAGE_BUDGET, every
	// kept cascade this device still holds data for with its answer size, key
	// size and Keep offline toggle, and every account with data here — its
	// figures from its `accounts` row, never from opening its database — each
	// with "Remove this account's data from this device".
	import { goto } from '$app/navigation';
	import * as Card from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { forgetSession, session } from '$lib/auth/session.svelte';
	import { removeAccountData, storageLine, type AccountRow } from '$lib/local/accounts';
	import type { UserDb } from '$lib/local/db';
	import { getMeta } from '$lib/local/meta';
	import { closeUserDb } from '$lib/local/open';
	import { ROW_STORAGE_BUDGET } from '$lib/sync/config';
	import { ROW_BYTES } from '$lib/sync/downloads';
	import { setKeepOffline } from '$lib/sync/keep';
	import { policy } from '$lib/sync/policy';
	import { downloadManager, syncState } from '$lib/sync/runtime.svelte';

	let { db }: { db: UserDb } = $props();

	interface Kept {
		id: string;
		name: string;
		answer: number;
		keys: number;
		user: boolean;
	}

	let accounts = $state<AccountRow[]>([]);
	let kept = $state<Kept[]>([]);
	let total = $state(0);

	// KB below a megabyte, so a small device's figures don't all read 0.0 MB.
	const mb = (b: number) => (b < 1_048_576 ? `${(b / 1024).toFixed(1)} KB` : `${(b / 1_048_576).toFixed(1)} MB`);

	async function refresh() {
		accounts = await storageLine();
		const pol = await policy(db);
		const sizes = await getMeta(db, 'sizes');
		const out: Kept[] = [];
		for (const id of new Set([...pol.userKept, ...pol.autoKept])) {
			const c = await db.get('cascades', id);
			const rows = await db.countFromIndex('quiz_questions', 'cascade_id', id);
			// Only while the device actually holds data for it.
			if (!c || (rows === 0 && !sizes[id]?.answer_bytes && !sizes[id]?.key_bytes)) continue;
			out.push({ id, name: c.name, answer: sizes[id]?.answer_bytes ?? 0, keys: sizes[id]?.key_bytes ?? 0, user: pol.userKept.has(id) });
		}
		kept = out;
		total = (await db.count('quiz_questions')) * ROW_BYTES + Object.values(sizes).reduce((n, s) => n + s.key_bytes, 0);
	}

	$effect(() => {
		void syncState.progress;
		void refresh();
	});

	async function toggle(k: Kept, on: boolean) {
		await setKeepOffline(db, k.id, on, downloadManager());
		await refresh();
	}

	async function remove(a: AccountRow) {
		if (a.user_id === session.userId) {
			// The signed-in account: clears the pointer and returns to the login page.
			closeUserDb();
			await removeAccountData(a.user_id);
			forgetSession();
			await goto('/login', { replaceState: true });
			return;
		}
		await removeAccountData(a.user_id);
		await refresh();
	}
</script>

<Card.Root>
	<Card.Header><Card.Title>Offline storage</Card.Title></Card.Header>
	<Card.Content class="grid gap-3 text-sm">
		<p>Rows and question keys: {mb(total)} of {mb(ROW_STORAGE_BUDGET)}.</p>
		{#if syncState.overBudgetByUserKept}
			<p>The cascades you keep offline hold this above the budget; turn Keep offline off to free space.</p>
		{/if}
		{#if syncState.status === 'no_room'}<p class="text-destructive">Not enough room on this device.</p>{/if}
		{#if kept.length}
			<table class="w-full">
				<thead><tr class="text-left text-muted-foreground"><th>Kept cascade</th><th>Answers</th><th>Keys</th><th>Keep offline</th></tr></thead>
				<tbody>
					{#each kept as k (k.id)}
						<tr>
							<td>{k.name}{#if !k.user}<span class="text-muted-foreground"> (kept automatically)</span>{/if}</td>
							<td>{mb(k.answer)}</td>
							<td>{mb(k.keys)}</td>
							<td><input type="checkbox" checked aria-label={`Keep ${k.name} offline`} onchange={(e) => toggle(k, e.currentTarget.checked)} /></td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
		<h3 class="pt-2 font-medium">Accounts with data on this device</h3>
		<ul class="grid gap-2">
			{#each accounts as a (a.user_id)}
				<li class="flex flex-wrap items-center justify-between gap-2">
					<span>{a.username}: rows and keys {mb(a.rows + a.keys)}, answers {mb(a.answer_bytes)}</span>
					<Button size="sm" variant="outline" onclick={() => remove(a)}>Remove this account's data from this device</Button>
				</li>
			{/each}
		</ul>
		<p class="text-muted-foreground">Some browsers, notably Safari, can clear site data after a period of not being used.</p>
	</Card.Content>
</Card.Root>
