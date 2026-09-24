<script lang="ts">
	// The Cascades page (PLAN.md § Cascades page): newest activity first, each
	// with its options in short form, a compact ladder, Complete and offline
	// badges, and a row menu; the header shows the cascade limit and the sync
	// status. Everything reads the local stores and works offline.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api';
	import { session } from '$lib/auth/session.svelte';
	import CascadeLadder from '$lib/components/CascadeLadder.svelte';
	import QuizOptionsForm from '$lib/components/QuizOptionsForm.svelte';
	import SyncStatus from '$lib/components/SyncStatus.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		allCascades,
		cascadeSummaries,
		CASCADE_LIMIT,
		keepHint,
		ladderText,
		type CascadeSummary
	} from '$lib/cascades/summary';
	import { applyLocally, type NewOp } from '$lib/local/apply';
	import { storeCreated, type Created } from '$lib/local/created';
	import type { UserDb } from '$lib/local/db';
	import { getMeta } from '$lib/local/meta';
	import { userDb } from '$lib/local/open';
	import { setKeepOffline } from '$lib/sync/keep';
	import { afterLocalWrite, downloadManager, syncState } from '$lib/sync/runtime.svelte';
	import { clampPrefill, type QuizOptions } from '$lib/options';

	let db = $state<UserDb | null>(null);
	let list = $state<CascadeSummary[]>([]);
	let count = $state(0);
	let opens = $state<Record<string, string>>({});
	/** Budget-dropped cascades: "kept automatically once opened" until they are opened again. */
	let dropped = $state<string[]>([]);
	let maxQuiz = $state(300_000);
	let editing = $state<{ id: string; count: number; options: QuizOptions } | null>(null);
	let error = $state<string | null>(null);
	let online = $state(true);

	async function refresh() {
		if (!db) return;
		online = navigator.onLine;
		list = await cascadeSummaries(db, online);
		count = (await allCascades(db)).length;
		opens = await getMeta(db, 'opens');
		dropped = await getMeta(db, 'budget_dropped');
	}

	onMount(() => {
		void (async () => {
			if (!session.userId) return;
			db = await userDb(session.userId, session.username);
			maxQuiz = (await getMeta(db, 'server')).max_quiz_questions;
			await refresh();
		})();
		const on = () => void refresh();
		addEventListener('online', on);
		addEventListener('offline', on);
		return () => {
			removeEventListener('online', on);
			removeEventListener('offline', on);
		};
	});

	// Syncs and downloads change what is shown.
	$effect(() => {
		void syncState.changed.tick;
		void syncState.progress;
		void refresh();
	});

	async function write(op: NewOp) {
		if (!db) return;
		await applyLocally(db, op);
		afterLocalWrite();
		await refresh();
	}

	function editOptions(s: CascadeSummary) {
		const c = s.cascade;
		editing = {
			id: c.id,
			count: c.question_count,
			options: { segment_size: clampPrefill(c.segment_size, maxQuiz), progression: c.progression, require_alphabetical: c.require_alphabetical }
		};
	}

	async function saveOptions() {
		if (!editing) return;
		const c = list.find((s) => s.cascade.id === editing!.id)!.cascade;
		const op: Record<string, unknown> = { type: 'set_cascade_options', cascade_id: c.id };
		if (editing.options.segment_size !== c.segment_size) op.segment_size = editing.options.segment_size;
		if (editing.options.progression !== c.progression) op.progression = editing.options.progression;
		if (editing.options.require_alphabetical !== c.require_alphabetical) op.require_alphabetical = editing.options.require_alphabetical;
		if (Object.keys(op).length > 2) await write(op as unknown as NewOp);
		editing = null;
	}

	async function startOver(s: CascadeSummary) {
		if (!db) return;
		error = null;
		const { device_id } = await getMeta(db, 'device');
		const body = { id: crypto.randomUUID(), source_quiz_id: crypto.randomUUID(), device_id, at: new Date().toISOString() };
		try {
			const created = await api.post<Created>(`/api/cascades/${s.cascade.id}/start-over`, body);
			await storeCreated(db, created);
			await goto(`/cascades/${body.id}`);
		} catch {
			error = 'Start over needs a connection.';
		}
	}

	async function toggleKeep(s: CascadeSummary, on: boolean) {
		if (!db) return;
		await setKeepOffline(db, s.cascade.id, on, downloadManager());
		await refresh();
	}

	function badge(s: CascadeSummary): string {
		switch (s.offline) {
			case 'available':
				return 'Available offline';
			case 'downloading':
				return `Downloading ${s.progress}%`;
			case 'answers_need_connection':
				return 'Answers need a connection';
			default:
				return 'Not downloaded on this device, open to download';
		}
	}
</script>

<main class="mx-auto max-w-4xl p-6">
	<header class="mb-6 flex flex-wrap items-center justify-between gap-4">
		<div>
			<h1 class="text-2xl font-semibold">Cascades</h1>
			<p class="text-sm text-muted-foreground">{count} of {CASCADE_LIMIT} cascades</p>
		</div>
		<div class="flex items-center gap-4">
			<SyncStatus />
			{#if count >= CASCADE_LIMIT}
				<span class="text-sm">At the limit — <a class="underline" href="/trash">Trash</a></span>
			{:else}
				<Button href="/cascades/new">New cascade</Button>
			{/if}
			<Button href="/trash" variant="outline">Trash</Button>
			<Button href="/account" variant="outline">{session.username || 'Account'}</Button>
		</div>
	</header>
	{#if error}<p class="mb-4 text-sm text-destructive" role="alert">{error}</p>{/if}
	{#if list.length === 0}
		<p class="text-muted-foreground">No cascades yet. <a class="underline" href="/cascades/new">Create one</a>.</p>
	{/if}
	<ul class="space-y-4">
		{#each list as s (s.cascade.id)}
			{@const hint = keepHint(s, !!opens[s.cascade.id] && !dropped.includes(s.cascade.id))}
			<li class="rounded-lg border p-4">
				<div class="flex flex-wrap items-baseline justify-between gap-2">
					<a class="text-lg font-medium underline-offset-2 hover:underline" href={`/cascades/${s.cascade.id}`}>{s.cascade.name}</a>
					<span class="text-sm text-muted-foreground">
						{s.cascade.quiz_type} · {s.cascade.lexicon} · clear at {s.cascade.clear_threshold}%{#if s.options} · {s.options}{/if}
					</span>
				</div>
				<p class="mt-1 text-sm text-muted-foreground" title={ladderText(s.levels)}>{ladderText(s.levels)}</p>
				<div class="mt-2"><CascadeLadder rows={s.levels} compact /></div>
				<div class="mt-2 flex flex-wrap gap-2 text-xs">
					{#if s.completedAt}<span class="rounded bg-muted px-2 py-1">Complete {new Date(s.completedAt).toLocaleDateString()}</span>{/if}
					<span class="rounded bg-muted px-2 py-1">{badge(s)}</span>
				</div>
				<div class="mt-3 flex flex-wrap items-center gap-2 text-sm">
					<Button size="sm" variant="outline" onclick={() => editOptions(s)}>Quiz options</Button>
					<Button size="sm" variant="outline" href={`/cascades/${s.cascade.id}/export`}>Export…</Button>
					<Button size="sm" variant="outline" disabled={count >= CASCADE_LIMIT} onclick={() => startOver(s)}>Start over</Button>
					<label class="flex items-center gap-1">
						<input type="checkbox" checked={s.keptByUser || s.keptAutomatically} onchange={(e) => toggleKeep(s, e.currentTarget.checked)} />
						Keep offline{#if hint}<span class="text-muted-foreground">{` (${hint})`}</span>{/if}
					</label>
					<Button size="sm" variant="outline" onclick={() => write({ type: 'trash_cascade', cascade_id: s.cascade.id })}>Move to Trash</Button>
				</div>
			</li>
		{/each}
	</ul>
</main>

{#if editing}
	<div class="fixed inset-0 z-40 flex items-center justify-center bg-black/30" role="presentation">
		<div class="w-full max-w-md space-y-4 rounded-lg bg-background p-6" role="dialog" aria-label="Quiz options">
			<h2 class="text-lg font-semibold">Quiz options for new quizzes</h2>
			<QuizOptionsForm bind:options={editing.options} questionCount={editing.count} maxQuizQuestions={maxQuiz} editing="cascade" />
			<div class="flex gap-2">
				<Button onclick={saveOptions}>Save</Button>
				<Button variant="outline" onclick={() => (editing = null)}>Cancel</Button>
			</div>
		</div>
	</div>
{/if}
