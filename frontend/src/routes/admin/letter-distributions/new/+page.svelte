<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Card from '$lib/components/ui/card';
	import UploadForm from '$lib/components/admin/UploadForm.svelte';

	let name = $state('');
</script>

<main class="mx-auto grid max-w-2xl gap-4 p-6">
	<a class="text-sm underline" href="/admin">← Catalog</a>
	<Card.Root>
		<Card.Header>
			<Card.Title>Upload a letter distribution</Card.Title>
			<Card.Description>A MAGPIE letter distribution CSV. The first line is the blank.</Card.Description>
		</Card.Header>
		<Card.Content>
			<UploadForm
				path="/api/admin/letter-distributions"
				accept=".csv,text/csv"
				fields={() => ({ name })}
				onFile={(f) => {
					// The name defaults to the file name without `.csv`.
					if (!name) name = f.name.replace(/\.csv$/i, '');
				}}
			>
				<div class="grid gap-1.5">
					<Label for="name">Name</Label>
					<Input id="name" bind:value={name} placeholder="english" />
				</div>
				{#snippet created(item)}
					Letter distribution <strong>{item.name}</strong> with {(item.tiles as unknown[]).length} tiles.
				{/snippet}
			</UploadForm>
		</Card.Content>
	</Card.Root>
</main>
