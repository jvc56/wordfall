<script lang="ts">
	// Segment size, progression and alphabetical order, with their inline
	// explanations (PLAN.md § Quiz options, § Frontend → QuizOptionsForm). The
	// same component serves the builder, the cascade's options dialog and the
	// quiz settings menu.
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { chainBound, segmentSizeError, type QuizOptions } from '$lib/options';

	let {
		options = $bindable(),
		questionCount = null,
		maxQuizQuestions = 300_000,
		editing = 'cascade',
		segmentChain = false,
		source = false,
		showAlphabetical = true
	}: {
		options: QuizOptions;
		/** The cascade's count in the builder and options dialog, the quiz's own in its menu. */
		questionCount?: number | null;
		maxQuizQuestions?: number;
		editing?: 'cascade' | 'quiz';
		segmentChain?: boolean;
		source?: boolean;
		showAlphabetical?: boolean;
	} = $props();

	let sizeText = $state(String(options.segment_size));
	let segmented = $state(options.segment_size > 0);

	let error = $derived(segmented ? segmentSizeError(Number(sizeText), maxQuizQuestions) : null);
	let levels = $derived(questionCount ? chainBound(questionCount, options.segment_size) : 0);

	$effect(() => {
		const size = segmented ? Number(sizeText) : 0;
		if (!segmentSizeError(size, maxQuizQuestions)) options.segment_size = size;
	});
</script>

<div class="grid gap-4">
	{#if segmentChain}
		<p class="text-sm text-muted-foreground">
			This quiz drills a run's misses: it always uses Drill progression and is never split into
			segments.
		</p>
	{:else}
		<div class="grid gap-1.5">
			<div class="flex items-center gap-2">
				<Checkbox id="segmented" bind:checked={segmented} />
				<Label for="segmented">Study in segments</Label>
				{#if segmented}
					<Input class="h-8 w-28" type="number" min="5" max={maxQuizQuestions} bind:value={sizeText} aria-label="Segment size" />
					<span class="text-sm">questions each</span>
				{/if}
			</div>
			<p class="text-xs text-muted-foreground">
				A segment of 40 means you study 40 at a time and drill what you missed before going on.
			</p>
			{#if error}<p class="text-sm text-destructive">{error}</p>{/if}
			{#if questionCount && levels > 2000}
				<p class="text-sm text-amber-400">
					One attempt could create up to {levels.toLocaleString()} drill levels, one for each segment with a miss.
				</p>
			{/if}
		</div>
		{#if !source}
			<fieldset class="grid gap-1.5">
				<legend class="text-sm font-medium">When a quiz is finished below the clear threshold</legend>
				<label class="flex items-start gap-2 text-sm">
					<input type="radio" value="ladder" bind:group={options.progression} class="mt-1" />
					<span><strong>Ladder</strong> — keep the quiz and go down a level with the misses; come back up to clear it.</span>
				</label>
				<label class="flex items-start gap-2 text-sm">
					<input type="radio" value="drill" bind:group={options.progression} class="mt-1" />
					<span><strong>Drill</strong> — replace the quiz with its misses and keep going until nothing is missed.</span>
				</label>
			</fieldset>
		{/if}
	{/if}
	{#if showAlphabetical}
		<div class="flex items-center gap-2">
			<Checkbox id="alphabetical" bind:checked={options.require_alphabetical} />
			<Label for="alphabetical">Typed answers must be in alphabetical order</Label>
		</div>
	{/if}
	{#if editing === 'quiz' && !segmentChain}
		<p class="text-xs text-muted-foreground">Changes apply to this quiz only, from the current card.</p>
	{/if}
</div>
