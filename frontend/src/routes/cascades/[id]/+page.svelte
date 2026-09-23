<script lang="ts">
	// The player for the cascade's deepest level (PLAN.md § Taking a quiz);
	// built in Phase 7e.
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
</script>

<main class="mx-auto max-w-3xl p-6">
	<p class="text-muted-foreground">Cascade <code>{page.params.id}</code> was created.</p>
	<Button href="/cascades" variant="outline" class="mt-4">Cascades</Button>
</main>
