<script lang="ts">
	// In Word List: paste or upload a file, one entry per line (PLAN.md §
	// Frontend → WordListEditor). Shows the count and how many entries are not
	// valid, recounting on every quiz type or lexicon change.
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import type { QuizType } from '$lib/filters';
	import type { Distribution } from '$lib/tiles';
	import { canonicalEntries, countInvalid, MAX_WORD_LIST_ENTRIES, type RowState } from '$lib/builder/form';

	let { row, dist, quizType }: { row: RowState; dist: Distribution | null; quizType: QuizType } =
		$props();

	let replacing = $state(false);
	let open = $derived(replacing || row.entries.length === 0);
	let text = $state('');
	let dropped = $state(0);

	let invalid = $derived(countInvalid(row.entries, dist, quizType));

	function apply(lines: string[]) {
		if (!dist) return;
		const r = canonicalEntries(lines, dist, quizType);
		row.entries = r.entries;
		row.entriesType = quizType;
		row.flag = null;
		dropped = r.invalid;
		replacing = false;
	}

	async function upload(e: Event) {
		const f = (e.currentTarget as HTMLInputElement).files?.[0];
		if (f) apply((await f.text()).split(/\r?\n/));
	}
</script>

<div class="grid gap-1.5">
	<p class="text-sm">
		{row.entries.length.toLocaleString()}
		{row.entries.length === 1 ? 'entry' : 'entries'}{#if invalid > 0}, <span class="text-amber-400"
				>{invalid.toLocaleString()} not valid here</span
			>{/if}{#if dropped > 0}; {dropped.toLocaleString()} lines could not be read{/if}.
		{#if row.entries.length > MAX_WORD_LIST_ENTRIES}
			<span class="text-destructive">At most {MAX_WORD_LIST_ENTRIES.toLocaleString()} in all.</span>
		{/if}
	</p>
	{#if open}
		<Textarea bind:value={text} rows={5} placeholder="One entry per line" class="font-mono text-xs" />
		<div class="flex flex-wrap gap-2">
			<Button type="button" size="sm" onclick={() => apply(text.split(/\r?\n/))} disabled={!dist}>Use this list</Button>
			<label class="cursor-pointer text-sm underline">
				Upload a file<input type="file" accept=".txt,.csv,text/plain" class="hidden" onchange={upload} />
			</label>
		</div>
	{:else}
		<Button type="button" size="sm" variant="outline" onclick={() => (replacing = true)}>Replace the list</Button>
	{/if}
</div>
