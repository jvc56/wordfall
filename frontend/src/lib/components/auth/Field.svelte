<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import type { FieldError } from '$lib/api';

	let {
		id,
		label,
		type = 'text',
		value = $bindable(''),
		errors = [],
		autocomplete
	}: {
		id: string;
		label: string;
		type?: string;
		value?: string;
		errors?: FieldError[];
		autocomplete?: HTMLInputElement['autocomplete'];
	} = $props();

	let mine = $derived(errors.filter((e) => e.field === id));
</script>

<div class="grid gap-1.5">
	<Label for={id}>{label}</Label>
	<Input {id} name={id} {type} bind:value {autocomplete} aria-invalid={mine.length > 0} />
	{#each mine as e}<p class="text-sm text-destructive">{e.message}</p>{/each}
</div>
