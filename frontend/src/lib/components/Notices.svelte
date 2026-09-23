<script lang="ts">
	// The rebase's notices (PLAN.md § Conflicts): shown until dismissed.
	import { dismissNotice, syncState } from '$lib/sync/runtime.svelte';
</script>

{#if syncState.notices.length}
	<div class="fixed right-4 bottom-4 z-50 max-w-sm space-y-2" role="status" aria-live="polite">
		{#each syncState.notices as n (n)}
			<div class="rounded-md border bg-background p-3 text-sm shadow">
				<p>{n.text}</p>
				<button class="mt-1 text-xs underline" onclick={() => dismissNotice(n)}>Dismiss</button>
			</div>
		{/each}
	</div>
{/if}
