<script lang="ts">
	// An AND / OR group: its operator switch, its ordered children (rows and
	// nested groups), and drag-and-drop between groups (PLAN.md § Frontend →
	// FilterGroup). The top of the builder is one of these.
	import { Button } from '$lib/components/ui/button';
	import FilterRow from './FilterRow.svelte';
	import Self from './FilterGroup.svelte';
	import { newGroup, newRow, type GroupState } from '$lib/builder/form';
	import { insertAfter, moveNode, removeNode } from '$lib/builder/tree-ops';
	import type { CeilingContext, ConditionType, LexiconInfo, QuizType } from '$lib/filters';
	import type { Distribution } from '$lib/tiles';

	let {
		group,
		root,
		top = false,
		quizType,
		ctx,
		dist,
		lexicons,
		lexicon,
		errors,
		defaultType,
		onremove
	}: {
		group: GroupState;
		root: GroupState;
		top?: boolean;
		quizType: QuizType;
		ctx: CeilingContext | null;
		dist: Distribution | null;
		lexicons: LexiconInfo[];
		lexicon: string;
		errors: Map<number, string>;
		defaultType: ConditionType;
		onremove?: () => void;
	} = $props();

	let dropIndex = $state<number | null>(null);

	function addRowAfter(key: number) {
		insertAfter(root, key, newRow(defaultType, quizType, ctx));
	}

	function addGroupAfter(key: number) {
		insertAfter(root, key, newGroup(group.op === 'and' ? 'or' : 'and', [newRow(defaultType, quizType, ctx)]));
	}

	function dragStart(e: DragEvent, key: number) {
		e.dataTransfer?.setData('application/x-wordfall-node', String(key));
		if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
		e.stopPropagation();
	}

	function drop(e: DragEvent, index: number) {
		e.preventDefault();
		e.stopPropagation();
		dropIndex = null;
		const key = Number(e.dataTransfer?.getData('application/x-wordfall-node'));
		if (key) moveNode(root, key, group.key, index);
	}
</script>

<div class="grid gap-2 {top ? '' : 'rounded-md border border-dashed p-2 pl-3'}" data-group-key={group.key}>
	<div class="flex items-center gap-2">
		{#if !top}
			<span
				class="cursor-grab select-none px-1 text-muted-foreground"
				draggable="true"
				ondragstart={(e) => dragStart(e, group.key)}
				role="button"
				tabindex="-1"
				aria-label="Drag group">⋮⋮</span
			>
		{/if}
		<div class="inline-flex overflow-hidden rounded-md border text-xs" role="radiogroup" aria-label="Group operator">
			{#each ['and', 'or'] as const as op (op)}
				<button
					type="button"
					role="radio"
					aria-checked={group.op === op}
					class="px-2 py-1 {group.op === op ? 'bg-primary text-primary-foreground' : ''}"
					onclick={() => (group.op = op)}>{op.toUpperCase()}</button
				>
			{/each}
		</div>
		<span class="text-xs text-muted-foreground">
			{group.op === 'and' ? 'every row must match' : 'any row may match'}
		</span>
		{#if !top}
			<Button type="button" size="sm" variant="ghost" class="ml-auto" onclick={onremove} aria-label="Remove group">−</Button>
		{/if}
	</div>
	{#if errors.get(group.key)}<p class="text-sm text-destructive">{errors.get(group.key)}</p>{/if}

	{#each group.children as child, i (child.key)}
		<div
			class="h-1 rounded {dropIndex === i ? 'bg-primary' : ''}"
			role="presentation"
			ondragover={(e) => {
				e.preventDefault();
				dropIndex = i;
			}}
			ondragleave={() => (dropIndex = null)}
			ondrop={(e) => drop(e, i)}
		></div>
		{#if child.kind === 'row'}
			<FilterRow
				row={child}
				{quizType}
				{ctx}
				{dist}
				{lexicons}
				{lexicon}
				error={errors.get(child.key) ?? null}
				onaddrow={() => addRowAfter(child.key)}
				onaddgroup={() => addGroupAfter(child.key)}
				onremove={() => removeNode(root, child.key)}
				ondragstart={(e) => dragStart(e, child.key)}
			/>
		{:else}
			<Self
				group={child}
				{root}
				{quizType}
				{ctx}
				{dist}
				{lexicons}
				{lexicon}
				{errors}
				{defaultType}
				onremove={() => removeNode(root, child.key)}
			/>
		{/if}
	{/each}
	<div
		class="h-2 rounded {dropIndex === group.children.length ? 'bg-primary' : ''}"
		role="presentation"
		ondragover={(e) => {
			e.preventDefault();
			dropIndex = group.children.length;
		}}
		ondragleave={() => (dropIndex = null)}
		ondrop={(e) => drop(e, group.children.length)}
	></div>
	{#if group.children.length === 0 || top}
		<div>
			<Button
				type="button"
				size="sm"
				variant="outline"
				onclick={() => group.children.push(newRow(defaultType, quizType, ctx))}>Add row</Button
			>
			<Button
				type="button"
				size="sm"
				variant="ghost"
				onclick={() => group.children.push(newGroup(group.op === 'and' ? 'or' : 'and', [newRow(defaultType, quizType, ctx)]))}
				>Add group</Button
			>
		</div>
	{/if}
</div>
