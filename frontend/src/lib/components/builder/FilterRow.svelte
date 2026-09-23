<script lang="ts">
	// One condition row (PLAN.md § Creating a cascade, step 3; § Frontend →
	// FilterRow), driven by the lib/filters.ts table.
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import TilePalette from './TilePalette.svelte';
	import WordListEditor from './WordListEditor.svelte';
	import {
		FILTERS,
		PARTS_OF_SPEECH,
		filterDef,
		type CeilingContext,
		type ConditionType,
		type LexiconInfo,
		type QuizType
	} from '$lib/filters';
	import { newRow, type RowState } from '$lib/builder/form';
	import type { Distribution } from '$lib/tiles';

	let {
		row,
		quizType,
		ctx,
		dist,
		lexicons,
		lexicon,
		error,
		onaddrow,
		onaddgroup,
		onremove,
		ondragstart
	}: {
		row: RowState;
		quizType: QuizType;
		ctx: CeilingContext | null;
		dist: Distribution | null;
		lexicons: LexiconInfo[];
		lexicon: string;
		error: string | null;
		onaddrow: () => void;
		onaddgroup: () => void;
		onremove: () => void;
		ondragstart: (e: DragEvent) => void;
	} = $props();

	let def = $derived(filterDef(row.type));
	let choices = $derived(FILTERS.filter((f) => f.appliesTo.includes(quizType) || f.type === row.type));
	let sameDist = $derived(
		lexicons.filter(
			(l) => l.letter_distribution === lexicons.find((x) => x.name === lexicon)?.letter_distribution
		)
	);
	let menu = $state(false);

	function changeType(e: Event) {
		const t = (e.currentTarget as HTMLSelectElement).value as ConditionType;
		const fresh = newRow(t, quizType, ctx);
		Object.assign(row, { ...fresh, key: row.key, negated: filterDef(t).negatable && row.negated });
	}

	function edited() {
		row.flag = null;
	}

	const inputClass = 'h-8';
</script>

<div
	class="grid gap-2 rounded-md border bg-card p-2 {error || row.flag ? 'border-destructive' : ''}"
	data-row-key={row.key}
>
	<div class="flex flex-wrap items-center gap-2">
		<span
			class="cursor-grab select-none px-1 text-muted-foreground"
			draggable="true"
			ondragstart={ondragstart}
			role="button"
			tabindex="-1"
			aria-label="Drag to move">⋮⋮</span
		>
		<label class="flex items-center gap-1 text-sm" title={def.negatable ? '' : `${def.label} cannot be negated`}>
			<Checkbox bind:checked={row.negated} disabled={!def.negatable} onCheckedChange={edited} />
			Not
		</label>
		<select
			class="h-8 rounded-md border bg-transparent px-2 text-sm"
			value={row.type}
			onchange={changeType}
			aria-label="Filter type"
		>
			{#each choices as f (f.type)}
				<option value={f.type}>{f.label}</option>
			{/each}
		</select>

		{#if def.kind === 'range' || def.kind === 'order' || def.kind === 'consists_of'}
			{#if def.kind === 'consists_of'}
				<Input class="{inputClass} w-32" placeholder="Tiles" bind:value={row.tiles} oninput={edited} aria-label="Tiles" />
			{/if}
			<Input class="{inputClass} w-24" type="number" bind:value={row.min} oninput={edited} aria-label="Minimum" />
			<span class="text-sm">to</span>
			<Input class="{inputClass} w-24" type="number" bind:value={row.max} oninput={edited} aria-label="Maximum" />
			{#if def.kind === 'consists_of'}<span class="text-sm">%</span>{/if}
			{#if def.kind === 'order'}
				<label class="flex items-center gap-1 text-sm" title="Take or leave a group of tied words together">
					<Checkbox bind:checked={row.lax} onCheckedChange={edited} /> Lax
				</label>
			{/if}
		{:else if def.kind === 'pattern'}
			<Input class="{inputClass} w-56 font-mono uppercase" bind:value={row.pattern} oninput={edited} aria-label="Pattern" />
		{:else if def.kind === 'tiles'}
			<Input class="{inputClass} w-40 font-mono uppercase" bind:value={row.tiles} oninput={edited} aria-label="Tiles" />
		{:else if def.kind === 'lexicon'}
			<select
				class="h-8 rounded-md border bg-transparent px-2 text-sm"
				bind:value={row.lexicon}
				onchange={edited}
				aria-label="Lexicon"
			>
				<option value="" disabled>Choose…</option>
				{#if row.lexicon && !sameDist.some((l) => l.name === row.lexicon)}
					<option value={row.lexicon}>{row.lexicon}</option>
				{/if}
				{#each sameDist as l (l.name)}
					<option value={l.name}>{l.name}</option>
				{/each}
			</select>
		{:else if def.kind === 'part_of_speech'}
			<select
				class="h-8 rounded-md border bg-transparent px-2 text-sm"
				bind:value={row.partOfSpeech}
				onchange={edited}
				aria-label="Part of speech"
			>
				{#each PARTS_OF_SPEECH as [v, label] (v)}
					<option value={v}>{label}</option>
				{/each}
			</select>
		{:else if def.kind === 'text'}
			<Input class="{inputClass} w-56" bind:value={row.text} oninput={edited} maxlength={500} aria-label="Text" />
		{:else if def.kind === 'leave_value'}
			<Input class="{inputClass} w-24" type="number" step="any" placeholder="min" bind:value={row.leaveMin} oninput={edited} aria-label="Minimum value" />
			<span class="text-sm">to</span>
			<Input class="{inputClass} w-24" type="number" step="any" placeholder="max" bind:value={row.leaveMax} oninput={edited} aria-label="Maximum value" />
		{/if}

		<div class="relative ml-auto flex gap-1">
			<Button type="button" size="sm" variant="ghost" onclick={() => (menu = !menu)} aria-label="Add">+</Button>
			{#if menu}
				<div class="absolute right-8 top-8 z-10 grid rounded-md border bg-popover p-1 shadow">
					<button type="button" class="rounded px-3 py-1 text-left text-sm hover:bg-accent" onclick={() => { menu = false; onaddrow(); }}>Add row</button>
					<button type="button" class="rounded px-3 py-1 text-left text-sm hover:bg-accent" onclick={() => { menu = false; onaddgroup(); }}>Add group</button>
				</div>
			{/if}
			<Button type="button" size="sm" variant="ghost" onclick={onremove} aria-label="Remove row">−</Button>
		</div>
	</div>

	{#if def.kind === 'entries'}
		<WordListEditor {row} {dist} {quizType} />
	{/if}
	{#if dist && (def.kind === 'pattern' || def.kind === 'tiles' || def.kind === 'consists_of')}
		<TilePalette
			{dist}
			blank={quizType === 'leave_value'}
			oninsert={(t) => {
				if (def.kind === 'pattern') row.pattern += t;
				else row.tiles += t;
				edited();
			}}
		/>
	{/if}
	{#if def.help}<p class="text-xs text-muted-foreground">{def.help}</p>{/if}
	{#if row.flag}<p class="text-sm text-destructive">{row.flag}</p>{/if}
	{#if error && error !== row.flag}<p class="text-sm text-destructive">{error}</p>{/if}
</div>
