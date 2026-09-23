<script lang="ts">
	import { onMount } from 'svelte';
	import { Label } from '$lib/components/ui/label';
	import * as Card from '$lib/components/ui/card';
	import UploadForm from '$lib/components/admin/UploadForm.svelte';
	import { loadAdminCatalog, type AdminCatalog } from '$lib/admin/catalog';

	let lexicon = $state('');
	let catalog = $state<AdminCatalog | null>(null);
	let choices = $derived((catalog?.lexicons ?? []).filter((l) => !l.has_leave_set));

	onMount(async () => {
		catalog = await loadAdminCatalog();
		lexicon = choices[0]?.name ?? '';
	});
</script>

<main class="mx-auto grid max-w-2xl gap-4 p-6">
	<a class="text-sm underline" href="/admin">← Catalog</a>
	<Card.Root>
		<Card.Header>
			<Card.Title>Upload leave values</Card.Title>
			<Card.Description>
				A comma-separated file: leave, value. A lexicon has at most one set; to replace it,
				delete the existing set first.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<UploadForm path="/api/admin/leave-sets" accept=".csv,text/csv" fields={() => ({ lexicon })}>
				<div class="grid gap-1.5">
					<Label for="lexicon">Lexicon</Label>
					<select id="lexicon" class="h-9 rounded-md border bg-transparent px-3 text-sm" bind:value={lexicon}>
						{#each choices as l (l.id)}
							<option value={l.name}>{l.name}</option>
						{/each}
					</select>
				</div>
				{#snippet created(item)}
					Leave values for <strong>{item.lexicon}</strong>: {Number(item.leave_count).toLocaleString()} leaves.
				{/snippet}
			</UploadForm>
		</Card.Content>
	</Card.Root>
</main>
