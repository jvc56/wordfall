<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { session } from '$lib/auth/session.svelte';
</script>

<main class="mx-auto max-w-4xl p-6">
	<header class="mb-6 flex items-center justify-between">
		<h1 class="text-2xl font-semibold">Cascades</h1>
		<Button href="/account" variant="outline">{session.username || 'Account'}</Button>
	</header>
</main>
