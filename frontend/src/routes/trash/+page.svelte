<script lang="ts">
	// The Trash (PLAN.md § Trash page): finished quizzes and trashed cascades,
	// grouped by cascade and collapsed, each group rendering its entries in
	// pages of 100 behind Show more, since one segmented attempt can leave tens
	// of thousands. Entries show the final score, cleared or replaced, and the
	// purge date, or "purges 30 days after this syncs" while it is provisional.
	import { onMount } from 'svelte';
	import { session } from '$lib/auth/session.svelte';
	import SyncStatus from '$lib/components/SyncStatus.svelte';
	import { Button } from '$lib/components/ui/button';
	import { canDeleteForever, PAGE, trashGroups, type TrashEntry, type TrashGroup } from '$lib/cascades/summary';
	import { applyLocally, type NewOp } from '$lib/local/apply';
	import type { UserDb } from '$lib/local/db';
	import { getMeta } from '$lib/local/meta';
	import { userDb } from '$lib/local/open';
	import { afterLocalWrite, syncState } from '$lib/sync/runtime.svelte';

	let db = $state<UserDb | null>(null);
	let groups = $state<TrashGroup[]>([]);
	let open = $state<Record<string, number>>({});
	let days = $state(30);

	async function refresh() {
		if (!db) return;
		groups = await trashGroups(db);
		days = (await getMeta(db, 'server')).trash_retention_days;
	}

	onMount(() => {
		void (async () => {
			if (!session.userId) return;
			db = await userDb(session.userId, session.username);
			await refresh();
		})();
	});

	$effect(() => {
		void syncState.changed.tick;
		void refresh();
	});

	async function write(op: NewOp) {
		if (!db) return;
		await applyLocally(db, op);
		afterLocalWrite();
		await refresh();
	}

	function restore(e: TrashEntry) {
		const seed = new BigUint64Array(1);
		crypto.getRandomValues(seed);
		return e.kind === 'cascade'
			? write({ type: 'restore_cascade', cascade_id: e.id })
			: write({ type: 'restore_quiz', quiz_id: e.id, shuffle_seed: seed[0].toString() });
	}

	function deleteForever(e: TrashEntry) {
		return e.kind === 'cascade'
			? write({ type: 'purge_cascade', cascade_id: e.id })
			: write({ type: 'purge_quiz', quiz_id: e.id });
	}

	const date = (iso: string) => new Date(iso).toLocaleDateString();
	// A group holding one entry is rendered expanded.
	const shown = (g: TrashGroup) => open[g.cascade.id] ?? (g.entries.length === 1 ? PAGE : 0);
</script>

<main class="mx-auto max-w-4xl p-6">
	<header class="mb-6 flex items-center justify-between">
		<h1 class="text-2xl font-semibold">Trash</h1>
		<div class="flex items-center gap-4">
			<SyncStatus />
			<Button href="/cascades" variant="outline">Cascades</Button>
		</div>
	</header>
	{#if groups.length === 0}<p class="text-muted-foreground">The Trash is empty.</p>{/if}
	<ul class="space-y-3">
		{#each groups as g (g.cascade.id)}
			{@const n = shown(g)}
			<li class="rounded-lg border p-4">
				<button class="flex w-full items-baseline justify-between text-left" onclick={() => (open[g.cascade.id] = n ? 0 : PAGE)}>
					<span class="font-medium">{g.trashed ? `${g.cascade.name} (cascade in the Trash)` : g.cascade.name}</span>
					<span class="text-sm text-muted-foreground">
						{`${g.entries.length} ${g.entries.length === 1 ? 'entry' : 'entries'}${g.earliest ? ` · purges from ${date(g.earliest)}` : ''}`}
					</span>
				</button>
				{#if n}
					<ul class="mt-3 divide-y text-sm">
						{#each g.entries.slice(0, n) as e (e.id)}
							<li class="flex flex-wrap items-center gap-3 py-2">
								<span class="min-w-40">
									{e.kind === 'cascade' ? 'The whole cascade' : `Level ${e.quiz!.level} · ${e.quiz!.question_count}`}
								</span>
								{#if e.score !== null}<span>{e.score}% · {e.outcome === 'replaced' ? 'replaced' : 'cleared'}</span>{/if}
								<span class="text-muted-foreground">
									{#if e.provisional}purges {days} days after this syncs{:else if e.purgesAt}purges {date(e.purgesAt)}{/if}
								</span>
								<span class="ml-auto flex gap-2">
									<Button size="sm" variant="outline" onclick={() => restore(e)}>Restore</Button>
									<Button size="sm" variant="outline" href={`/cascades/${g.cascade.id}/export${e.kind === 'quiz' ? `?quiz=${e.id}` : ''}`}>Export…</Button>
									{#if canDeleteForever(e)}
										<Button size="sm" variant="outline" onclick={() => deleteForever(e)}>Delete forever</Button>
									{/if}
								</span>
								{#if g.notHere}<span class="w-full text-xs text-muted-foreground">Downloads when online.</span>{/if}
							</li>
						{/each}
					</ul>
					{#if g.entries.length > n}
						<Button size="sm" variant="ghost" onclick={() => (open[g.cascade.id] = n + PAGE)}>Show more</Button>
					{/if}
				{/if}
			</li>
		{/each}
	</ul>
</main>
