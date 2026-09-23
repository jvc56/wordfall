<script lang="ts">
	import * as Card from '$lib/components/ui/card';
	import type { Snippet } from 'svelte';

	let { title, description, children, footer }: {
		title: string;
		description?: string;
		children: Snippet;
		footer?: Snippet;
	} = $props();
</script>

<main class="flex min-h-screen items-center justify-center p-4">
	<Card.Root class="w-full max-w-sm">
		<Card.Header>
			<Card.Title class="text-xl">{title}</Card.Title>
			{#if description}<Card.Description>{description}</Card.Description>{/if}
		</Card.Header>
		<Card.Content>{@render children()}</Card.Content>
		{#if footer}<Card.Footer class="text-sm text-muted-foreground">{@render footer()}</Card.Footer>{/if}
	</Card.Root>
</main>
