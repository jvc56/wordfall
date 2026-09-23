<script lang="ts">
	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { initSession, session } from '$lib/auth/session.svelte';

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
	});

	$effect(() => {
		if (!session.ready) return;
		const path = page.url.pathname;
		if (!session.userId && !PUBLIC.includes(path)) goto('/login', { replaceState: true });
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>Wordfall</title>
</svelte:head>

<div class="min-h-screen bg-background text-foreground">
	{#if session.updatedInAnotherTab}
		<!-- PLAN.md § On the device: the old build's tab keeps running and never reloads by itself. -->
		<p role="status" class="bg-muted px-4 py-2 text-center text-sm">
			Wordfall was updated in another tab — reload to continue
		</p>
	{/if}
	{#if session.ready}
		{@render children()}
	{/if}
</div>
