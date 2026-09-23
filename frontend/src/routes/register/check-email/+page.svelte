<script lang="ts">
	import { page } from '$app/state';
	import AuthCard from '$lib/components/auth/AuthCard.svelte';
	let email = $derived(page.url.searchParams.get('email'));
</script>

<AuthCard title="Check your email">
	<p class="text-sm">
		We sent a confirmation link{email ? ` to ${email}` : ''}. Follow it within 24 hours to finish
		creating your account.
	</p>
	{#snippet footer()}<a class="underline" href="/login">Back to log in</a>{/snippet}
</AuthCard>
