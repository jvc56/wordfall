<script lang="ts">
	// The preferences (PLAN.md § Preferences). Each change is a
	// `set_preferences` operation, so it works offline and syncs; the three
	// defaults only prefill the builder, and a segment size is clamped to the
	// cap `/api/auth/me` reported.
	import * as Card from '$lib/components/ui/card';
	import { applyLocally, type NewOp } from '$lib/local/apply';
	import type { UserDb } from '$lib/local/db';
	import type { PreferencesRow } from '$lib/local/rows';
	import { segmentSizeError } from '$lib/options';
	import { afterLocalWrite } from '$lib/sync/runtime.svelte';

	let { db, prefs, maxQuiz, onchange }: { db: UserDb; prefs: PreferencesRow; maxQuiz: number; onchange: () => void } = $props();

	let sizeText = $state('');
	let sizeError = $state<string | null>(null);
	$effect(() => {
		sizeText = String(prefs.default_segment_size);
	});

	async function set(field: keyof PreferencesRow, value: unknown) {
		await applyLocally(db, { type: 'set_preferences', [field]: value } as NewOp);
		afterLocalWrite();
		onchange();
	}

	function setThreshold(v: string) {
		const n = Number(v);
		if (Number.isInteger(n) && n >= 1 && n <= 100) void set('default_clear_threshold', n);
	}

	function setSegment() {
		const n = Number(sizeText);
		sizeError = Number.isFinite(n) ? segmentSizeError(n, maxQuiz) : 'Use a whole number.';
		if (!sizeError) void set('default_segment_size', n);
	}
</script>

<Card.Root>
	<Card.Header><Card.Title>Preferences</Card.Title></Card.Header>
	<Card.Content class="grid gap-3 text-sm">
		<label class="flex items-center justify-between gap-2">
			Default clear threshold (%)
			<input class="w-20 rounded border px-2 py-1" type="number" min="1" max="100" value={prefs.default_clear_threshold} onchange={(e) => setThreshold(e.currentTarget.value)} />
		</label>
		<label class="flex items-center justify-between gap-2">
			Default segment size (0 is off)
			<input class="w-28 rounded border px-2 py-1" bind:value={sizeText} onchange={setSegment} inputmode="numeric" />
		</label>
		{#if sizeError}<p class="text-destructive">{sizeError}</p>{/if}
		<label class="flex items-center justify-between gap-2">
			Default progression
			<select value={prefs.default_progression} onchange={(e) => set('default_progression', e.currentTarget.value)}>
				<option value="ladder">Ladder</option>
				<option value="drill">Drill</option>
			</select>
		</label>
		<label class="flex items-center gap-2">
			<input type="checkbox" checked={prefs.default_require_alphabetical} onchange={(e) => set('default_require_alphabetical', e.currentTarget.checked)} />
			Default alphabetical order
		</label>
		<label class="flex items-center justify-between gap-2">
			Leave value decimal places
			<select value={String(prefs.leave_value_decimals)} onchange={(e) => set('leave_value_decimals', Number(e.currentTarget.value))}>
				{#each [0, 1, 2, 3] as d (d)}<option value={String(d)}>{d}</option>{/each}
			</select>
		</label>
		<label class="flex items-center gap-2">
			<input type="checkbox" checked={prefs.anagram_show_definitions} onchange={(e) => set('anagram_show_definitions', e.currentTarget.checked)} />
			Show definitions with anagrams
		</label>
		<label class="flex items-center gap-2">
			<input type="checkbox" checked={prefs.anagram_show_hooks} onchange={(e) => set('anagram_show_hooks', e.currentTarget.checked)} />
			Show hooks with anagrams
		</label>
		<label class="flex items-center justify-between gap-2">
			Anagram answer mode
			<select value={prefs.anagram_answer_mode} onchange={(e) => set('anagram_answer_mode', e.currentTarget.value)}>
				<option value="flashcard">Flashcard</option>
				<option value="typed">Typed</option>
			</select>
		</label>
	</Card.Content>
</Card.Root>
