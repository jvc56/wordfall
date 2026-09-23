<script lang="ts">
	// The levels of a cascade (PLAN.md § Frontend → CascadeLadder): sizes,
	// attempts, last scores, the current run of a segmented level, which level
	// is current, a pending level shown as "waiting for download", and per-level
	// Quiz options and Export… actions. `compact` is the Cascades page's form.
	import { compactItems, ladderItems, type LevelRow } from '$lib/player/ladder';

	let {
		rows,
		compact = false,
		onOptions,
		onExport
	}: {
		rows: LevelRow[];
		compact?: boolean;
		onOptions?: (row: LevelRow) => void;
		onExport?: (row: LevelRow) => void;
	} = $props();

	let expanded = $state(false);
	let items = $derived(compact ? compactItems(rows, expanded) : ladderItems(rows, expanded));
</script>

<ol class="space-y-1 text-sm" aria-label="Ladder">
	{#each items as item, i (item.kind === 'level' ? item.row.quiz.id : `more-${i}`)}
		{#if item.kind === 'more'}
			<li>
				<button class="text-muted-foreground underline" onclick={() => (expanded = true)}>
					{compact ? `+${item.count} more` : `Show more (${item.count} levels)`}
				</button>
			</li>
		{:else}
			{@const r = item.row}
			<li class="flex items-baseline gap-2" class:font-semibold={r.current} data-level={r.level}>
				<span>Level {r.level}</span>
				{#if !compact}
					<span class="text-muted-foreground">{r.size}</span>
					<span class="text-muted-foreground">attempt {r.attempt}</span>
					{#if r.lastScore !== null}<span class="text-muted-foreground">last {r.lastScore}%</span>{/if}
				{/if}
				{#if r.downloading}
					<span class="text-muted-foreground">waiting for download</span>
				{:else if r.current}
					<span>◀ now</span>
					{#if r.run && !compact}<span class="text-muted-foreground">{r.run}</span>{/if}
				{:else}
					<span class="text-muted-foreground">waiting</span>
				{/if}
				{#if !compact && onOptions}
					<button class="ml-auto underline" onclick={() => onOptions?.(r)}>Quiz options</button>
				{/if}
				{#if !compact && onExport}
					<button class="underline" onclick={() => onExport?.(r)}>Export…</button>
				{/if}
			</li>
		{/if}
	{/each}
</ol>
