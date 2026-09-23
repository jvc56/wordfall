<script lang="ts">
	// The answer Show / Next reveals (PLAN.md § Taking a quiz → Answers):
	// anagrams one per line in the card's order, hooks Zyzzyva-style and
	// lower-cased only for single ASCII letters, definitions beneath; a
	// definition's text; a leave value with its sign at the user's decimals.
	// Unfound words are highlighted after a typed-mode reveal.
	import TileText from '$lib/components/TileText.svelte';
	import { leaveValueText } from '$lib/cascade/leave';
	import type { PreferencesRow } from '$lib/local/rows';

	interface Word {
		word: string;
		front_hooks?: string;
		back_hooks?: string;
		definition?: string;
	}

	let {
		quizType,
		answer,
		prefs,
		found = null
	}: {
		quizType: string;
		answer: unknown;
		prefs: PreferencesRow;
		/** Words found in typed mode; the others are highlighted. */
		found?: string[] | null;
	} = $props();

	let words = $derived(Array.isArray(answer) ? (answer as Word[]) : []);
	let missingDefinitions = $derived(
		quizType === 'anagram' && prefs.anagram_show_definitions && words.some((w) => w.definition === undefined)
	);
</script>

{#if quizType === 'anagram'}
	<ul class="space-y-1 text-center">
		{#each words as w (w.word)}
			<li class:text-destructive={found !== null && !found.includes(w.word)}>
				{#if prefs.anagram_show_hooks && w.front_hooks}<TileText text={w.front_hooks} lower class="mr-2 text-muted-foreground" />{/if}<TileText
					text={w.word}
					class="font-semibold tracking-wide"
				/>{#if prefs.anagram_show_hooks && w.back_hooks}<TileText text={w.back_hooks} lower class="ml-2 text-muted-foreground" />{/if}
				{#if prefs.anagram_show_definitions && w.definition}
					<div class="text-sm text-muted-foreground">{w.definition}</div>
				{/if}
			</li>
		{/each}
	</ul>
	{#if missingDefinitions}
		<p class="mt-2 text-center text-sm text-muted-foreground">Definitions are downloading.</p>
	{/if}
{:else if quizType === 'definition'}
	<p class="text-center">{String(answer)}</p>
{:else}
	<p class="text-center text-2xl font-semibold">{leaveValueText(Number(answer), prefs.leave_value_decimals, true)}</p>
{/if}
