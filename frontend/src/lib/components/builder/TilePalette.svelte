<script lang="ts">
	// Clickable tiles for the current distribution, shown under tile inputs
	// when it has any multi-character or non-ASCII tile. A tile is inserted
	// whole with a space on each side, so it is never re-split.
	import type { Distribution } from '$lib/tiles';

	let { dist, oninsert, blank = false }: { dist: Distribution; oninsert: (text: string) => void; blank?: boolean } =
		$props();
	let tiles = $derived(dist.tiles.map((t, i) => ({ t, i })).filter(({ i }) => i > 0 || blank));
</script>

{#if !dist.plainAZ}
	<div class="mt-1 flex flex-wrap gap-1" role="group" aria-label="Tile palette">
		{#each tiles as { t, i } (i)}
			<button
				type="button"
				class="min-w-7 rounded border bg-muted px-1.5 py-0.5 text-sm hover:bg-accent"
				onclick={() => oninsert(` ${t.letter} `)}>{t.letter}</button
			>
		{/each}
	</div>
{/if}
