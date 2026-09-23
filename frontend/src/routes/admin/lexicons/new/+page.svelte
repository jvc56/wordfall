<script lang="ts">
	import { onMount } from 'svelte';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Card from '$lib/components/ui/card';
	import UploadForm from '$lib/components/admin/UploadForm.svelte';
	import { loadAdminCatalog, type AdminCatalog } from '$lib/admin/catalog';

	let name = $state('');
	let distribution = $state('');
	let catalog = $state<AdminCatalog | null>(null);

	onMount(async () => {
		catalog = await loadAdminCatalog();
		distribution = catalog.letter_distributions[0]?.name ?? '';
	});
</script>

<main class="mx-auto grid max-w-2xl gap-4 p-6">
	<a class="text-sm underline" href="/admin">← Catalog</a>
	<Card.Root>
		<Card.Header>
			<Card.Title>Upload a lexicon</Card.Title>
			<Card.Description>
				A tab-separated file: word, playability, definition. Words in MAGPIE notation.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<UploadForm
				path="/api/admin/lexicons"
				accept=".tsv,.txt,text/tab-separated-values"
				fields={() => ({ name, letter_distribution: distribution })}
			>
				<div class="grid gap-1.5">
					<Label for="name">Name</Label>
					<Input id="name" bind:value={name} placeholder="CSW24" />
				</div>
				<div class="grid gap-1.5">
					<Label for="distribution">Letter distribution</Label>
					<select id="distribution" class="h-9 rounded-md border bg-transparent px-3 text-sm" bind:value={distribution}>
						{#each catalog?.letter_distributions ?? [] as d (d.id)}
							<option value={d.name}>{d.name}</option>
						{/each}
					</select>
				</div>
				{#snippet created(item)}
					Lexicon <strong>{item.name}</strong> with {Number(item.word_count).toLocaleString()} words.
					It appears in the cascade builder once every server has loaded it.
				{/snippet}
			</UploadForm>
		</Card.Content>
	</Card.Root>
</main>
