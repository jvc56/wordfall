<script lang="ts">
	import { goto } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import { session } from '$lib/auth/session.svelte';

	$effect(() => {
		if (session.userId) goto('/cascades', { replaceState: true });
	});
	// Arrived: the next sign-out goes to /login as usual.
	$effect(() => {
		if (session.toLanding) session.toLanding = false;
	});
</script>

{#if !session.userId}
	<main class="mx-auto flex min-h-screen max-w-2xl flex-col justify-center gap-6 p-8">
		<h1 class="text-4xl font-semibold tracking-tight">Wordfall</h1>
		<p class="text-lg text-muted-foreground">
			Word study for crossword game players. Describe the words you want with filters, and
			study them as a cascade of flashcard quizzes — online or off.
		</p>
		<div class="flex gap-3">
			<Button href="/register">Create an account</Button>
			<Button href="/login" variant="outline">Log in</Button>
		</div>
	</main>
{/if}
