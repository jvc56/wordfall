<script lang="ts">
	// The player for the cascade's deepest level (PLAN.md § Taking a quiz):
	// the desktop layout with side rails for a fine pointer at least 900 px
	// wide, tap zones otherwise; the three actions from mouse, wheel, keys and
	// taps; typed mode; the settings menu with preferences and this quiz's
	// options; banners, the completion screen, and a trashed cascade's Restore.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '$lib/api';
	import { session } from '$lib/auth/session.svelte';
	import CascadeLadder from '$lib/components/CascadeLadder.svelte';
	import QuizOptionsForm from '$lib/components/QuizOptionsForm.svelte';
	import SyncStatus from '$lib/components/SyncStatus.svelte';
	import TileText from '$lib/components/TileText.svelte';
	import AnswerView from '$lib/components/player/AnswerView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { runIndicator, runIndicatorText } from '$lib/cascade/rules';
	import { applyLocally, quizState, type NewOp } from '$lib/local/apply';
	import { storeCreated, type Created } from '$lib/local/created';
	import type { UserDb } from '$lib/local/db';
	import { getMeta, recordOpen } from '$lib/local/meta';
	import { userDb } from '$lib/local/open';
	import { defaultPreferences, preferencesView } from '$lib/local/preferences';
	import type { AttemptRow, PreferencesRow } from '$lib/local/rows';
	import type { RwTx } from '$lib/local/view';
	import { PLAYER_LOCK } from '$lib/sync/downloads';
	import { afterLocalWrite, ensureCascade, fetchCard, syncState } from '$lib/sync/runtime.svelte';
	import { actionFor, Debounce, inputOwns, keyStroke, mouseStroke, wheelStroke, type Action } from '$lib/player/bindings';
	import { Player, type PlayerView } from '$lib/player/controller';
	import { levelRows, type LevelRow } from '$lib/player/ladder';
	import type { QuizOptions } from '$lib/options';

	const id = $derived(page.params.id as string);

	let db = $state<UserDb | null>(null);
	let player = $state<Player | null>(null);
	let v = $state<PlayerView | null>(null);
	let prefs = $state<PreferencesRow>(defaultPreferences());
	let attempts = $state<AttemptRow[]>([]);
	let desktop = $state(true);
	let landscape = $state(false);
	let settingsOpen = $state(false);
	let typedText = $state('');
	let typedInput = $state<HTMLInputElement | null>(null);
	let typedFocused = $state(false);
	let quizOptions = $state<QuizOptions | null>(null);
	let maxQuiz = $state(300_000);
	let busy = $state(false);

	const actions = new Debounce(120);
	const wheel = new Debounce(150);

	let rows = $derived<LevelRow[]>(v?.cascade ? levelRows(v.quizzes, v.cascade.depth, attempts) : []);
	let run = $derived.by(() => {
		if (!v?.quiz) return null;
		const ri = runIndicator(quizState(v.quiz));
		return ri ? runIndicatorText(ri) : null;
	});

	async function refreshSide() {
		if (!db) return;
		prefs = await preferencesView(db);
		attempts = await db.getAllFromIndex('quiz_attempts', 'cascade_id', id);
	}

	onMount(() => {
		const fine = matchMedia('(pointer: fine) and (min-width: 900px)');
		const land = matchMedia('(orientation: landscape)');
		const layout = () => {
			desktop = fine.matches;
			landscape = land.matches;
		};
		layout();
		fine.addEventListener('change', layout);
		land.addEventListener('change', layout);
		let release: (() => void) | null = null;
		let unsub: (() => void) | null = null;
		let cancelled = false;
		(async () => {
			if (!session.userId) return;
			const d = await userDb(session.userId, session.username);
			if (cancelled) return;
			db = d;
			maxQuiz = (await getMeta(d, 'server')).max_quiz_questions;
			// Navigating to the player is an open (§ Downloads).
			const tx = d.transaction(['meta'], 'readwrite');
			await recordOpen(tx as unknown as RwTx, id, new Date().toISOString());
			await tx.done;
			// Each player tab holds a Web Lock named for its cascade, so no drop pass takes it.
			if (navigator.locks) {
				void navigator.locks.request(PLAYER_LOCK + id, () => new Promise<void>((r) => (release = r)));
			}
			syncState.openCascade = id;
			const p = new Player(d, id, {
				prefs: () => preferencesView(d),
				ensure: (cid) => ensureCascade(d, cid),
				fetchCard: (cid, idx) => fetchCard(d, cid, idx),
				afterWrite: () => {
					afterLocalWrite();
					void refreshSide();
				}
			});
			unsub = p.subscribe((x) => (v = x));
			player = p;
			await refreshSide();
			await p.load();
		})();
		return () => {
			cancelled = true;
			fine.removeEventListener('change', layout);
			land.removeEventListener('change', layout);
			unsub?.();
			release?.();
			if (syncState.openCascade === id) syncState.openCascade = null;
		};
	});

	// Step 5 of the rebase: refresh if what the player shows changed.
	$effect(() => {
		const c = syncState.changed;
		if (c.tick && player && c.ids.includes(id)) {
			void refreshSide();
			void player.load(true);
		}
	});

	// A download made progress: the card takes what it lacked.
	$effect(() => {
		if (syncState.progress && player) void player.refreshCard();
	});

	async function act(a: Action) {
		if (!player || busy || !actions.pass(a, performance.now())) return;
		const card = v?.card;
		// Before the reveal, Show / Next on a typed card focuses the input instead of giving up.
		if (a === 'show_next' && card?.typed && !card.revealed && !typedFocused) {
			typedInput?.focus();
			return;
		}
		busy = true;
		try {
			if (a === 'show_next') await player.showNext();
			else if (a === 'toggle_grade') {
				await player.toggle();
				navigator.vibrate?.(15);
			} else await player.previous();
			if (a !== 'toggle_grade') typedText = '';
		} finally {
			busy = false;
		}
	}

	function onKey(e: KeyboardEvent) {
		if (!v || v.status !== 'ready' || settingsOpen) return;
		const t = e.target as HTMLElement | null;
		const inTyped = t === typedInput;
		// Text fields outside the quiz area keep their keys.
		if (!inTyped && t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
		if (inTyped && inputOwns(e)) return;
		const a = actionFor(prefs.bindings, keyStroke(e));
		if (!a) return;
		e.preventDefault();
		void act(a);
	}

	function onMouse(e: MouseEvent) {
		if ((e.target as HTMLElement).closest('input,button,a')) return;
		const s = mouseStroke(e);
		if (!s) return;
		e.preventDefault();
		const a = actionFor(prefs.bindings, s);
		if (a) void act(a);
	}

	function onWheel(e: WheelEvent) {
		const s = wheelStroke(e);
		if (!s) return;
		const a = actionFor(prefs.bindings, s);
		if (!a) return; // an unbound direction scrolls as usual
		e.preventDefault();
		if (wheel.pass(s.code, performance.now())) void act(a);
	}

	async function onTypedKey(e: KeyboardEvent) {
		if (e.key !== 'Enter' || !player) return;
		e.preventDefault();
		const text = typedText;
		const r = await player.enter(text);
		if (r && (r.kind === 'found' || r.kind === 'out_of_order' || r.kind === 'already' || r.kind === 'wrong')) typedText = '';
		if (r?.kind === 'empty') typedText = '';
	}

	async function write(op: NewOp) {
		if (!db) return;
		await applyLocally(db, op);
		afterLocalWrite();
		await refreshSide();
		await player?.load(true);
	}

	async function setPref(field: string, value: unknown) {
		await write({ type: 'set_preferences', [field]: value } as NewOp);
		if (field === 'anagram_answer_mode') await player?.modeChanged();
	}

	function openSettings() {
		const q = v?.quiz;
		if (q) {
			quizOptions = {
				segment_size: q.segment_size,
				progression: q.progression,
				require_alphabetical: q.require_alphabetical
			};
		}
		settingsOpen = true;
	}

	async function saveQuizOptions() {
		const q = v?.quiz;
		if (!q || !quizOptions) return;
		const op: Record<string, unknown> = { type: 'set_quiz_options', quiz_id: q.id };
		if (quizOptions.segment_size !== q.segment_size) op.segment_size = quizOptions.segment_size;
		if (quizOptions.progression !== q.progression) op.progression = quizOptions.progression;
		if (quizOptions.require_alphabetical !== q.require_alphabetical) op.require_alphabetical = quizOptions.require_alphabetical;
		if (Object.keys(op).length > 2) await write(op as unknown as NewOp);
		settingsOpen = false;
	}

	async function restore() {
		await write({ type: 'restore_cascade', cascade_id: id });
	}

	async function moveToTrash() {
		await write({ type: 'trash_cascade', cascade_id: id });
		await goto('/cascades');
	}

	async function startOver() {
		if (!db || !session.userId) return;
		const { device_id } = await getMeta(db, 'device');
		const body = { id: crypto.randomUUID(), source_quiz_id: crypto.randomUUID(), device_id, at: new Date().toISOString() };
		const created = await api.post<Created>(`/api/cascades/${id}/start-over`, body);
		await storeCreated(db, created);
		await goto(`/cascades/${body.id}`);
	}

	function keepStudying() {
		if (v) v = { ...v, completion: null };
	}
</script>

<svelte:window onkeydown={onKey} />

{#snippet question()}
	{#if v?.card}
		{@const card = v.card}
		{#if card.key === null}
			<p class="text-center text-muted-foreground">This question needs a connection.</p>
		{:else}
			<div class="text-center text-4xl font-semibold tracking-widest"><TileText text={card.key} /></div>
			{#if card.typed && !card.revealed}
				<div class="mx-auto mt-6 max-w-sm space-y-2">
					<p class="text-center text-sm text-muted-foreground">
						{card.typed.found.length} of {player?.words(card).length ?? 0} found
					</p>
					<input
						bind:this={typedInput}
						bind:value={typedText}
						onkeydown={onTypedKey}
						onfocus={() => (typedFocused = true)}
						onblur={() => (typedFocused = false)}
						class="w-full rounded-md border px-3 py-2 text-center uppercase"
						aria-label="Type an answer"
						autocomplete="off"
						autocapitalize="characters"
						spellcheck="false"
					/>
					{#if card.note}<p class="text-center text-sm text-muted-foreground">{card.note}</p>{/if}
					{#if v.quiz?.require_alphabetical && card.typed.found.length}
						<p class="text-center text-xs text-muted-foreground">after {card.typed.found.at(-1)}</p>
					{/if}
					<ul class="flex flex-wrap justify-center gap-2 text-sm">
						{#each card.typed.found as w (w)}<li><TileText text={w} /></li>{/each}
					</ul>
					<ul class="flex flex-wrap justify-center gap-2 text-sm text-destructive">
						{#each card.typed.wrong as w, i (i)}<li>{w}</li>{/each}
					</ul>
				</div>
			{/if}
			{#if card.revealed}
				<div class="mt-6">
					{#if card.answer === null}
						<p class="text-center text-muted-foreground">Answer needs a connection.</p>
					{:else}
						<AnswerView quizType={v.cascade!.quiz_type} answer={card.answer} {prefs} found={card.typed?.found ?? null} />
					{/if}
				</div>
				<p class="mt-6 text-center text-3xl font-bold" class:text-green-700={card.grade === 'correct'} class:text-destructive={card.grade === 'missed'}>
					{card.grade === 'correct' ? '✓ Correct' : '✗ Missed'}
				</p>
			{:else if card.toggled}
				<p class="mt-6 text-center text-sm text-muted-foreground">Marked missed</p>
			{/if}
		{/if}
		{#if v.distributionMissing}
			<p class="mt-2 text-center text-sm text-muted-foreground">Typed mode needs a connection for this cascade’s tiles.</p>
		{/if}
		{#if v.message}
			<p class="mt-4 text-center text-sm text-muted-foreground" role="status">{v.message}</p>
		{/if}
		{#if v.storage}
			<div class="mt-2 text-center text-sm">
				Free space by turning Keep offline off or removing another account’s data on the
				<a class="underline" href="/account">Account page</a>.
				<Button size="sm" variant="outline" onclick={() => player?.retryStorage()}>Try again</Button>
			</div>
		{/if}
	{/if}
{/snippet}

{#snippet status()}
	{#if !v || v.status === 'loading'}
		<p class="text-muted-foreground">Loading…</p>
	{:else if v.status === 'downloading'}
		<p class="text-muted-foreground">Downloading this level…</p>
	{:else if v.status === 'needs_connection'}
		<p class="text-muted-foreground">This cascade needs a connection to download.</p>
	{:else if v.status === 'gone'}
		<p class="text-muted-foreground">This cascade is not on this device.</p>
		<Button href="/cascades" variant="outline" class="mt-4">Cascades</Button>
	{:else if v.status === 'trashed'}
		<p>This cascade is in the Trash.</p>
		<Button class="mt-4" onclick={restore}>Restore</Button>
		<div class="mt-6"><CascadeLadder {rows} /></div>
	{/if}
{/snippet}

{#snippet side()}
	{#if v?.quiz && v.cascade}
		<div class="space-y-1 text-sm">
			<p class="font-semibold">Level {v.quiz.level} of {v.cascade.depth}</p>
			<p>attempt {v.quiz.attempt}</p>
			<p>{(v.card?.pos ?? v.quiz.cursor) + 1} / {v.quiz.question_count}</p>
			<p>{v.quiz.correct_count} ✓ {v.quiz.missed_count} ✗</p>
			<p>clear at {v.cascade.clear_threshold}%</p>
			{#if run}<p>{run}</p>{/if}
		</div>
		<div class="mt-4"><CascadeLadder {rows} /></div>
	{/if}
	<Button variant="ghost" class="mt-4" onclick={openSettings}>⚙ Preferences</Button>
{/snippet}

{#if desktop}
	<div class="grid h-screen grid-cols-[12rem_1fr_14rem]">
		<nav class="flex flex-col gap-2 border-r p-4 text-sm">
			<a href="/cascades" class="font-semibold">Wordfall</a>
			<a href="/cascades">Cascades</a>
			<a href="/cascades/new">New</a>
			<a href="/trash">Trash</a>
			<a href="/account">Account</a>
			<div class="mt-auto"><SyncStatus /></div>
		</nav>
		<!-- The quiz area: the only place mouse controls act; no selection, no context menu. -->
		<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
		<div
			role="application"
			class="relative flex min-w-[60vw] flex-col items-center justify-center overflow-y-auto p-8 select-none"
			aria-label="Quiz area"
			onmousedown={(e) => e.button === 1 && e.preventDefault()}
			onmouseup={onMouse}
			oncontextmenu={(e) => e.preventDefault()}
			onauxclick={(e) => e.preventDefault()}
			onwheel={onWheel}
		>
			{#if v?.banner}<p class="absolute top-4 rounded-md bg-muted px-4 py-2 text-sm" role="status">{v.banner}</p>{/if}
			{#if v?.status === 'ready'}{@render question()}{:else}{@render status()}{/if}
		</div>
		<aside class="border-l p-4">{@render side()}</aside>
	</div>
{:else}
	<div class="flex h-screen flex-col">
		<header class="flex items-center justify-between border-b px-3 py-2 text-sm">
			<a href="/cascades" aria-label="Menu">☰</a>
			<span>
				{#if v?.quiz && v.cascade}
					L{v.quiz.level} · {(v.card?.pos ?? v.quiz.cursor) + 1}/{v.quiz.question_count} · {v.cascade.clear_threshold}%{#if run} · {run}{/if}
				{/if}
			</span>
			<button onclick={openSettings} aria-label="Settings">⚙</button>
		</header>
		{#if v?.status === 'ready'}
			<div class="relative flex flex-1 select-none" class:flex-col={!landscape} style="touch-action: manipulation">
				{#if v.banner}<p class="absolute top-2 left-2 right-2 z-10 rounded-md bg-muted px-3 py-2 text-sm" role="status">{v.banner}</p>{/if}
				<button class="text-muted-foreground" style={landscape ? 'width:20%' : 'height:15%'} onclick={() => act('previous')}>↶ Previous</button>
				<div
					class="overflow-y-auto border-y p-4"
					style={landscape ? 'width:55%' : 'height:60%'}
					role="button"
					tabindex="0"
					onclick={(e) => !(e.target as HTMLElement).closest('input,button,a') && act('show_next')}
					onkeydown={() => undefined}
				>
					{@render question()}
				</div>
				<button class="text-muted-foreground" style={landscape ? 'width:25%' : 'height:25%'} onclick={() => act('toggle_grade')}>✓ ⇄ ✗ Toggle</button>
				{#if typedFocused}
					<!-- The on-screen keyboard covers the zones: three buttons sit above it. -->
					<div class="fixed right-0 bottom-0 left-0 flex justify-around border-t bg-background p-2">
						<Button variant="outline" onmousedown={(e: MouseEvent) => e.preventDefault()} onclick={() => act('previous')}>Previous</Button>
						<Button variant="outline" onmousedown={(e: MouseEvent) => e.preventDefault()} onclick={() => act('show_next')}>Show / Next</Button>
						<Button variant="outline" onmousedown={(e: MouseEvent) => e.preventDefault()} onclick={() => act('toggle_grade')}>Toggle</Button>
					</div>
				{/if}
			</div>
		{:else}
			<div class="p-6">{@render status()}</div>
		{/if}
	</div>
{/if}

{#if settingsOpen}
	<div class="fixed inset-0 z-40 flex justify-end bg-black/30" role="presentation" onclick={() => (settingsOpen = false)}>
		<div
			class="h-full w-full max-w-md space-y-4 overflow-y-auto bg-background p-6"
			role="dialog"
			aria-label="Settings"
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}
			onkeydown={(e) => e.key === 'Escape' && (settingsOpen = false)}
		>
			<h2 class="text-lg font-semibold">Preferences</h2>
			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" checked={prefs.anagram_show_definitions} onchange={(e) => setPref('anagram_show_definitions', e.currentTarget.checked)} />
				Show definitions with anagrams
			</label>
			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" checked={prefs.anagram_show_hooks} onchange={(e) => setPref('anagram_show_hooks', e.currentTarget.checked)} />
				Show hooks with anagrams
			</label>
			<label class="flex items-center gap-2 text-sm">
				Anagram answer mode
				<select value={prefs.anagram_answer_mode} onchange={(e) => setPref('anagram_answer_mode', e.currentTarget.value)}>
					<option value="flashcard">Flashcard</option>
					<option value="typed">Typed</option>
				</select>
			</label>
			<label class="flex items-center gap-2 text-sm">
				Leave value decimal places
				<select value={String(prefs.leave_value_decimals)} onchange={(e) => setPref('leave_value_decimals', Number(e.currentTarget.value))}>
					{#each [0, 1, 2, 3] as d (d)}<option value={String(d)}>{d}</option>{/each}
				</select>
			</label>
			<p class="text-sm"><a class="underline" href="/account#controls">Controls</a> are edited on the Account page.</p>
			{#if v?.quiz && quizOptions}
				<h2 class="pt-4 text-lg font-semibold">This quiz’s options</h2>
				<QuizOptionsForm
					bind:options={quizOptions}
					questionCount={v.quiz.question_count}
					maxQuizQuestions={maxQuiz}
					editing="quiz"
					segmentChain={v.quiz.segment_chain}
					source={v.quiz.origin === 'source'}
					showAlphabetical={v.cascade?.quiz_type === 'anagram'}
				/>
				<Button onclick={saveQuizOptions}>Save options</Button>
			{/if}
			<Button variant="outline" onclick={() => (settingsOpen = false)}>Close</Button>
		</div>
	</div>
{/if}

{#if v?.completion}
	<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
		<div class="max-w-sm space-y-4 rounded-lg bg-background p-6 text-center" role="dialog" aria-label="Cascade complete">
			<h2 class="text-xl font-semibold">Cascade complete</h2>
			<p>After {v.completion.levels} levels and {v.completion.attempts} attempts.</p>
			<div class="flex flex-col gap-2">
				<Button onclick={keepStudying}>Keep studying</Button>
				<Button variant="outline" onclick={startOver}>Start over</Button>
				<Button variant="outline" onclick={moveToTrash}>Move to Trash</Button>
			</div>
		</div>
	</div>
{/if}
