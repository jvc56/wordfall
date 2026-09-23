<script lang="ts">
	// The sync status in the left rail (PLAN.md § Frontend: synced, syncing,
	// offline with pending work, log in to sync, reload to keep syncing, not
	// enough room on this device, signed out in another tab).
	import { session } from '$lib/auth/session.svelte';
	import { applyUpdate } from '$lib/sw/client.svelte';
	import { syncState } from '$lib/sync/runtime.svelte';

	let text = $derived.by(() => {
		if (session.signedOutInAnotherTab) return 'Signed out in another tab';
		if (session.needsLogin || syncState.status === 'needs_login') return 'Log in to sync';
		switch (syncState.status) {
			case 'reload':
				return 'Reload to keep syncing';
			case 'no_room':
				return 'Not enough room on this device';
			case 'offline':
				return 'Offline — changes will sync';
			case 'syncing':
				return 'Syncing…';
			default:
				return 'Synced';
		}
	});
</script>

<div class="flex items-center gap-2 text-sm" role="status">
	<span class="inline-block h-2 w-2 rounded-full" class:bg-green-600={text === 'Synced'} class:bg-muted-foreground={text !== 'Synced'}></span>
	{#if syncState.status === 'reload'}
		<button class="underline" onclick={applyUpdate}>{text}</button>
	{:else if text === 'Log in to sync'}
		<a class="underline" href="/login">{text}</a>
	{:else}
		<span>{text}</span>
	{/if}
</div>
