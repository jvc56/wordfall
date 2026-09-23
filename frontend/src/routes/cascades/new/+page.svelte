<script lang="ts">
	// The cascade builder (PLAN.md § Creating a cascade), laid out like
	// Zyzzyva's Search tab. Needs a connection: searching runs on the server.
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import FilterGroup from '$lib/components/builder/FilterGroup.svelte';
	import QuizOptionsForm from '$lib/components/QuizOptionsForm.svelte';
	import TileText from '$lib/components/TileText.svelte';
	import { api, ApiError } from '$lib/api';
	import { session } from '$lib/auth/session.svelte';
	import { loadDistribution, loadLexicons } from '$lib/catalog';
	import { sendWithRetry } from '$lib/builder/create';
	import {
		changeLexicon,
		changeQuizType,
		fromWire,
		newGroup,
		newRow,
		nodeAt,
		saveBlocker,
		toWire,
		type GroupState
	} from '$lib/builder/form';
	import { PreviewScheduler, type PreviewState } from '$lib/builder/preview';
	import {
		QUIZ_TYPES,
		QUIZ_TYPE_LABELS,
		parseTree,
		summaryName,
		type CeilingContext,
		type LexiconInfo,
		type QuizType
	} from '$lib/filters';
	import { storeCreated, type Created } from '$lib/local/created';
	import { getMeta } from '$lib/local/meta';
	import { preferencesView } from '$lib/local/preferences';
	import { allCascades, CASCADE_LIMIT, CASCADE_WARNING } from '$lib/cascades/summary';
	import { userDb } from '$lib/local/open';
	import { DEFAULT_OPTIONS, clampPrefill, type QuizOptions } from '$lib/options';
	import { PREVIEW_MANUAL_ENTRIES } from '$lib/sync/config';
	import type { Distribution } from '$lib/tiles';

	const cap = $derived(session.me?.max_quiz_questions ?? 300_000);

	let quizType = $state<QuizType>('anagram');
	let lexicons = $state<LexiconInfo[]>([]);
	let lexicon = $state('');
	let dist = $state<Distribution | null>(null);
	let root = $state<GroupState>(newGroup('and', [newRow('length', 'anagram', null)]));
	let threshold = $state(80);
	let cascadeCount = $state(0);
	let options = $state<QuizOptions>({ ...DEFAULT_OPTIONS });
	let name = $state('');
	let nameEdited = $state(false);
	let preview = $state<PreviewState | null>(null);
	let creating = $state(false);
	let busy = $state(false);
	let createError = $state<string | null>(null);
	let loadError = $state<string | null>(null);

	let lexiconInfo = $derived(lexicons.find((l) => l.name === lexicon) ?? null);
	let ctx = $derived<CeilingContext | null>(
		lexiconInfo && dist ? { quizType, lexicon: lexiconInfo, maxTileValue: dist.maxValue } : null
	);
	let wire = $derived(toWire(root, quizType, dist, ctx));
	let manual = $derived(wire.entries > PREVIEW_MANUAL_ENTRIES);

	/** Client errors, plus the preview's 400 errors mapped from paths to rows. */
	let errors = $derived.by(() => {
		const m = new Map(wire.errors);
		for (const e of preview?.errors ?? []) {
			const n = nodeAt(root, e.path);
			if (n && !m.has(n.key)) m.set(n.key, e.message);
		}
		return m;
	});

	const scheduler = new PreviewScheduler(
		(body) => api.post('/api/search/preview', body),
		(s) => (preview = s)
	);
	onDestroy(() => scheduler.dispose());

	onMount(async () => {
		if (session.userId) {
			// The threshold and options are prefilled from the preferences view, the
			// segment size clamped to the cap in `meta` (§ Creating a cascade, § Quiz options).
			const db = await userDb(session.userId, session.username);
			const p = await preferencesView(db);
			const metaCap = (await getMeta(db, 'server')).max_quiz_questions;
			threshold = p.default_clear_threshold;
			options = {
				segment_size: clampPrefill(p.default_segment_size, metaCap),
				progression: p.default_progression,
				require_alphabetical: p.default_require_alphabetical
			};
			cascadeCount = (await allCascades(db)).length;
		}
		options.segment_size = clampPrefill(options.segment_size, cap);
		try {
			lexicons = await loadLexicons();
			lexicon = lexicons[0]?.name ?? '';
		} catch {
			loadError = 'Could not load the lexicons. Creating a cascade needs a connection.';
		}
	});

	// The lexicon's distribution, and In Lexicon rows flagged on a change.
	$effect(() => {
		const info = lexiconInfo;
		if (!info) return;
		changeLexicon(root, lexicons, info.name);
		loadDistribution(info.letter_distribution).then(
			(d) => (dist = d),
			() => (loadError = 'Could not load the letter distribution.')
		);
	});

	let lastType: QuizType = 'anagram';
	function setQuizType(q: QuizType) {
		if (q === lastType) return;
		lastType = q;
		quizType = q;
		changeQuizType(root, q);
		if (q === 'leave_value' && lexiconInfo && lexiconInfo.leave_count === null) {
			lexicon = lexicons.find((l) => l.leave_count !== null)?.name ?? lexicon;
		}
	}

	$effect(() => {
		if (!nameEdited && lexicon) name = summaryName(lexicon, wire.tree);
	});

	$effect(() => {
		const ready = lexicon && dist && wire.errors.size === 0;
		scheduler.update(ready ? { lexicon, quiz_type: quizType, filters: wire.tree } : null, manual);
	});

	async function create() {
		if (!session.userId) return;
		createError = null;
		creating = true;
		// This device's id for the account, from the per-user `meta` store.
		const { device_id } = await getMeta(await userDb(session.userId, session.username), 'device');
		// Minted once per press and reused for every retry of it.
		const body = {
			id: crypto.randomUUID(),
			source_quiz_id: crypto.randomUUID(),
			device_id,
			at: new Date().toISOString(),
			name,
			lexicon,
			quiz_type: quizType,
			clear_threshold: threshold,
			segment_size: options.segment_size,
			progression: options.progression,
			require_alphabetical: options.require_alphabetical,
			filters: wire.tree
		};
		try {
			const created = await sendWithRetry(() => api.post<Created>('/api/cascades', body), { onBusy: (b) => (busy = b) });
			// Into the base at once: a cascade starts downloading the moment it is created.
			await storeCreated(await userDb(session.userId, session.username), created);
			await goto(`/cascades/${body.id}`);
		} catch (e) {
			if (e instanceof ApiError && e.code === 'cascade_limit') {
				const b = e.body as { limit: number };
				createError = `You have reached the limit of ${b.limit} cascades. Empty the Trash to make room.`;
			} else if (e instanceof ApiError && e.code === 'empty') createError = 'The search matched nothing.';
			else if (e instanceof ApiError && e.code === 'over_cap')
				createError = `The search matched ${(e.body as { count: number }).count.toLocaleString()} questions, more than ${cap.toLocaleString()}. Narrow it.`;
			else if (e instanceof ApiError && e.status === 400)
				createError = e.fieldErrors.length ? e.fieldErrors.map((f) => f.message).join(' ') : 'Fix the rows marked in red.';
			else createError = 'The cascade could not be created. Check your connection.';
		} finally {
			creating = false;
			busy = false;
		}
	}

	// Save Search… and Load Search…
	let saveOpen = $state(false);
	let saveName = $state('');
	let saveMessage = $state<string | null>(null);
	let saveBusy = $state(false);
	let blocker = $derived(saveBlocker(root, wire.errors));

	async function save(overwrite = false, id = crypto.randomUUID()) {
		saveMessage = null;
		const body = { id, name: saveName, quiz_type: quizType, filters: wire.tree, overwrite };
		try {
			await sendWithRetry(() => api.post('/api/searches', body), { onBusy: (b) => (saveBusy = b) });
			saveOpen = false;
		} catch (e) {
			if (e instanceof ApiError && e.code === 'name_taken') {
				if (confirm(`Replace your saved search "${saveName}"?`)) await save(true, id);
			} else if (e instanceof ApiError && e.code === 'saved_search_limit') {
				saveMessage = `You have ${(e.body as { limit: number }).limit} saved searches, the most you can keep. Delete one first.`;
			} else if (e instanceof ApiError && e.status === 400) {
				saveMessage = (e.body as { errors?: { message: string }[] })?.errors?.map((x) => x.message).join(' ') ?? 'Invalid.';
			} else saveMessage = 'The search could not be saved.';
		}
	}

	interface SavedSummary {
		id: string;
		name: string;
		quiz_type: QuizType;
		word_list_entries: number;
	}
	let loadOpen = $state(false);
	let saved = $state<SavedSummary[]>([]);
	let loadMessage = $state<string | null>(null);

	async function openLoad() {
		loadOpen = true;
		loadMessage = null;
		try {
			saved = await api.get<SavedSummary[]>('/api/searches');
		} catch {
			loadMessage = 'Could not load your saved searches.';
		}
	}

	async function load(s: SavedSummary) {
		loadMessage = 'Loading…';
		try {
			const full = await sendWithRetry(() =>
				api.get<{ quiz_type: QuizType; filters: unknown }>(`/api/searches/${s.id}`)
			);
			const loaded = fromWire(parseTree(full.filters), full.quiz_type);
			// Flag against the stored type, from the load alone.
			changeQuizType(loaded, quizType);
			changeLexicon(loaded, lexicons, lexicon);
			root = loaded;
			nameEdited = false;
			loadOpen = false;
		} catch {
			loadMessage = 'Could not load that search.';
		}
	}
</script>

<main class="mx-auto grid max-w-5xl gap-4 p-4 md:p-6">
	<header class="flex items-center justify-between">
		<h1 class="text-2xl font-semibold">New cascade</h1>
		<Button href="/cascades" variant="outline">Cancel</Button>
	</header>
	{#if loadError}<p class="text-sm text-destructive">{loadError}</p>{/if}

	<Card.Root>
		<Card.Content class="grid gap-4 pt-6">
			<div class="flex flex-wrap gap-6">
				<fieldset class="grid gap-1">
					<legend class="text-sm font-medium">Quiz type</legend>
					<div class="flex gap-3">
						{#each QUIZ_TYPES as q (q)}
							<label class="flex items-center gap-1 text-sm">
								<input type="radio" name="quiz_type" checked={quizType === q} onchange={() => setQuizType(q)} />
								{QUIZ_TYPE_LABELS[q]}
							</label>
						{/each}
					</div>
				</fieldset>
				<div class="grid gap-1">
					<Label for="lexicon">Lexicon</Label>
					<select id="lexicon" class="h-9 rounded-md border bg-transparent px-2 text-sm" bind:value={lexicon}>
						{#each lexicons as l (l.name)}
							{@const disabled = quizType === 'leave_value' && l.leave_count === null}
							<option value={l.name} {disabled} title={disabled ? `${l.name} has no leave values` : ''}>
								{l.name}
							</option>
						{/each}
					</select>
				</div>
			</div>

			<div class="grid gap-2">
				<div class="flex items-center justify-between">
					<h2 class="font-medium">Filters</h2>
					<div class="flex gap-2">
						<Button type="button" size="sm" variant="outline" onclick={openLoad}>Load Search…</Button>
						<Button
							type="button"
							size="sm"
							variant="outline"
							onclick={() => {
								saveMessage = blocker;
								saveOpen = true;
							}}>Save Search…</Button
						>
					</div>
				</div>
				<FilterGroup
					group={root}
					{root}
					top
					{quizType}
					{ctx}
					{dist}
					{lexicons}
					{lexicon}
					{errors}
					defaultType="length"
				/>
			</div>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header><Card.Title>Preview</Card.Title></Card.Header>
		<Card.Content class="grid gap-2">
			{#if manual}
				<p class="text-sm text-muted-foreground">
					A word list this long is previewed only when you ask.
					<Button type="button" size="sm" variant="outline" onclick={() => scheduler.previewNow()}>Preview</Button>
				</p>
			{/if}
			{#if preview?.count != null}
				<p class="text-lg">
					{preview.count.toLocaleString()} questions
					{#if preview.overCap}<span class="text-sm text-destructive">— more than {cap.toLocaleString()}, the most a cascade can hold</span>{/if}
					{#if preview.busy}<span class="text-sm text-amber-400">the server is busy</span>{/if}
					{#if preview.loading}<span class="text-sm text-muted-foreground">…</span>{/if}
				</p>
				<div class="flex flex-wrap gap-x-3 gap-y-1 font-mono text-sm">
					{#each preview.sample as key (key)}<TileText text={key} />{/each}
				</div>
			{:else if preview?.busy}
				<p class="text-sm text-amber-400">the server is busy</p>
			{:else if wire.errors.size}
				<p class="text-sm text-muted-foreground">Fix the rows marked in red to see a preview.</p>
			{/if}
			{#if preview?.failed}<p class="text-sm text-destructive">{preview.failed}</p>{/if}
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Content class="grid gap-4 pt-6">
			<div class="grid max-w-xs gap-1">
				<Label for="threshold">Clear threshold (%)</Label>
				<Input id="threshold" type="number" min="1" max="100" bind:value={threshold} />
				<p class="text-xs text-muted-foreground">Score at least this much on a quiz to clear it.</p>
			</div>
			<QuizOptionsForm
				bind:options
				questionCount={preview?.count ?? null}
				maxQuizQuestions={cap}
				showAlphabetical={quizType === 'anagram'}
			/>
			<div class="grid gap-1">
				<Label for="name">Cascade name</Label>
				<Input id="name" bind:value={name} maxlength={200} oninput={() => (nameEdited = true)} />
			</div>
			{#if createError}<p class="text-sm text-destructive">{createError}</p>{/if}
			{#if cascadeCount >= CASCADE_LIMIT}
				<p class="text-sm">You have {cascadeCount} of {CASCADE_LIMIT} cascades. <a class="underline" href="/trash">Empty the Trash</a> to make room.</p>
			{:else if cascadeCount >= CASCADE_WARNING}
				<p class="text-sm text-muted-foreground">{cascadeCount} of {CASCADE_LIMIT} cascades used.</p>
			{/if}
			<div>
				<Button
					type="button"
					onclick={create}
					disabled={creating || cascadeCount >= CASCADE_LIMIT || !lexicon || !dist || wire.errors.size > 0 || !name.trim() || threshold < 1 || threshold > 100}
				>
					{busy ? 'the server is busy' : creating ? 'Creating…' : 'Create Cascade'}
				</Button>
			</div>
		</Card.Content>
	</Card.Root>
</main>

<Dialog.Root bind:open={saveOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Save Search</Dialog.Title>
			<Dialog.Description>Saves the filters only, never the results, so they work with any lexicon.</Dialog.Description>
		</Dialog.Header>
		<div class="grid gap-2">
			<Input bind:value={saveName} placeholder="Name" maxlength={100} />
			{#if saveMessage}<p class="text-sm text-destructive">{saveMessage}</p>{/if}
		</div>
		<Dialog.Footer>
			<Button type="button" onclick={() => save()} disabled={!!blocker || !saveName.trim() || saveBusy}>
				{saveBusy ? 'the server is busy' : 'Save'}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={loadOpen}>
	<Dialog.Content>
		<Dialog.Header><Dialog.Title>Load Search</Dialog.Title></Dialog.Header>
		{#if loadMessage}<p class="text-sm">{loadMessage}</p>{/if}
		<ul class="grid max-h-96 gap-1 overflow-auto">
			{#each saved as s (s.id)}
				<li>
					<button type="button" class="w-full rounded px-2 py-1 text-left hover:bg-accent" onclick={() => load(s)}>
						{s.name}
						<span class="text-xs text-muted-foreground">
							· {QUIZ_TYPE_LABELS[s.quiz_type]}{s.word_list_entries ? ` · ${s.word_list_entries.toLocaleString()} word-list entries` : ''}
						</span>
					</button>
				</li>
			{:else}
				<li class="text-sm text-muted-foreground">No saved searches yet.</li>
			{/each}
		</ul>
	</Dialog.Content>
</Dialog.Root>
