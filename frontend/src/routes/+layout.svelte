<script lang="ts">
	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { initSession, session } from '$lib/auth/session.svelte';
	import { startSync } from '$lib/sync/runtime.svelte';
	import { applyUpdate, updates, watchUpdates } from '$lib/sw/client.svelte';
	import Notices from '$lib/components/Notices.svelte';

	let { children } = $props();

	// Every page except these requires a logged-in user (PLAN.md § Accounts).
	const PUBLIC = [
		'/',
		'/login',
		'/register',
		'/register/check-email',
		'/confirm-email',
		'/reset-password',
		'/reset-password/confirm'
	];

	onMount(() => {
		initSession();
		void watchUpdates();
	});

	// Sync runs while an account is signed in and this tab still belongs to it.
	$effect(() => {
		const id = session.userId;
		if (!id || session.signedOutInAnotherTab) return;
		return startSync(id);
	});

	$effect(() => {
		if (!session.ready) return;
		const path = page.url.pathname;
		// A deleted account lands on the landing page, not the login page.
		if (!session.userId && session.toLanding) {
			if (path !== '/') goto('/', { replaceState: true });
			return;
		}
		if (!session.userId && !PUBLIC.includes(path)) goto('/login', { replaceState: true });
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>Wordfall</title>
</svelte:head>

<div class="min-h-screen bg-background text-foreground">
	{#if updates.ready && !updates.updatedElsewhere}
		<p role="status" class="bg-muted px-4 py-2 text-center text-sm">
			A new version is ready.
			<button class="underline" onclick={applyUpdate}>Reload</button>
		</p>
	{/if}
	{#if session.updatedInAnotherTab || updates.updatedElsewhere}
		<!-- PLAN.md § On the device: the old build's tab keeps running and never reloads by itself. -->
		<p role="status" class="bg-muted px-4 py-2 text-center text-sm">
			Wordfall was updated in another tab — reload to continue
		</p>
	{/if}
	<Notices />
	{#if session.ready}
		{@render children()}
	{/if}
</div>
