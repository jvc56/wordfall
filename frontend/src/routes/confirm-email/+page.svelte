<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import AuthCard from '$lib/components/auth/AuthCard.svelte';
	import FormError from '$lib/components/auth/FormError.svelte';
	import { ApiError } from '$lib/api';
	import { authApi } from '$lib/auth/client';
	import { describe } from '$lib/auth/errors';

	let error = $state<string | null>(null);

	onMount(async () => {
		const code = page.url.searchParams.get('code');
		if (!code) {
			error = 'This link has no confirmation code.';
			return;
		}
		try {
			await authApi.confirmEmail(code);
			await goto('/login?confirmed=1', { replaceState: true });
		} catch (e) {
			error =
				e instanceof ApiError && e.status === 400
					? 'This confirmation link has expired or was already used. Register again to get a new one.'
					: describe(e);
		}
	});
</script>

<AuthCard title="Confirming your email">
	{#if error}<FormError message={error} />{:else}<p class="text-sm">One moment…</p>{/if}
</AuthCard>
