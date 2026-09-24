<script lang="ts">
	// The Export… dialog (PLAN.md § Exporting words): what, which questions,
	// format, lines or columns, order; a live count of the questions the
	// selection holds. Built on the device from local data in a worker when it
	// has what the file needs, after materialising what it can; otherwise a
	// token from POST …/export-token and a hidden frame navigated to the
	// download, so a failure never replaces the app. Offline, an export that
	// needs the server offers the questions alone.
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, ApiError } from '$lib/api';
	import { session } from '$lib/auth/session.svelte';
	import { Button } from '$lib/components/ui/button';
	import { exportFilename, selection, type Column, type Which } from '$lib/export/format';
	import { localInput, serverChoices, type DeviceChoices } from '$lib/export/local';
	import { APPLY_STORES } from '$lib/local/apply';
	import type { UserDb } from '$lib/local/db';
	import { recordOpen } from '$lib/local/meta';
	import { userDb } from '$lib/local/open';
	import { preferencesView } from '$lib/local/preferences';
	import type { CascadeRow, QuizRow } from '$lib/local/rows';
	import * as view from '$lib/local/view';
	import type { RwTx } from '$lib/local/view';
	import { DownloadManager } from '$lib/sync/downloads';
	import { downloadManager, ensureCascade } from '$lib/sync/runtime.svelte';

	const id = $derived(page.params.id as string);
	let db = $state<UserDb | null>(null);
	let cascade = $state<CascadeRow | null>(null);
	let quizzes = $state<QuizRow[]>([]);
	let choices = $state<DeviceChoices>({
		scope: 'cascade',
		which: 'all',
		format: 'txt',
		lines: 'answers',
		columns: ['question', 'answer', 'grade'],
		order: 'study',
		decimals: 1
	});
	let count = $state<number | null>(null);
	let entries = $state<number | null>(null);
	let message = $state<string | null>(null);
	let offerQuestions = $state(false);
	let busy = $state(false);
	let frame = $state<HTMLIFrameElement | null>(null);

	const WHICH: { v: Which; label: string }[] = [
		{ v: 'all', label: 'All' },
		{ v: 'correct', label: 'Correct' },
		{ v: 'missed', label: 'Missed' },
		{ v: 'ungraded', label: 'Not yet answered' }
	];
	let columnsOffered = $derived<Column[]>(
		cascade?.quiz_type === 'leave_value' ? ['question', 'answer', 'grade'] : ['question', 'answer', 'definition', 'hooks', 'grade']
	);

	onMount(() => {
		void (async () => {
			if (!session.userId) return;
			const d = await userDb(session.userId, session.username);
			db = d;
			const tx = d.transaction(APPLY_STORES) as unknown as RwTx;
			cascade = (await view.cascade(tx, id)) ?? null;
			quizzes = (await view.quizzesOf(tx, id)).sort((a, b) => a.level - b.level);
			const prefs = await preferencesView(d);
			const quiz = page.url.searchParams.get('quiz');
			choices = {
				...choices,
				scope: quiz ? 'quiz' : 'cascade',
				quiz_id: quiz ?? undefined,
				lines: cascade?.quiz_type === 'anagram' ? 'answers' : 'questions',
				decimals: prefs.leave_value_decimals
			};
			await recount();
		})();
	});

	/** The questions the selection holds, from the `questions` store; the entries too when the cards are here. */
	async function recount() {
		if (!db || !cascade) return;
		const q = await localInput(db, cascade, { ...choices, format: 'txt', lines: 'questions' });
		count = 'input' in q ? selection(q.input).length : null;
		const a = await localInput(db, cascade, { ...choices, format: 'txt', lines: 'answers' });
		entries = 'input' in a && cascade.quiz_type === 'anagram' ? selection(a.input).reduce((n, e) => n + (e.q.words?.length ?? 0), 0) : null;
	}

	$effect(() => {
		void JSON.stringify(choices);
		void recount();
	});

	function save(blob: Blob, name: string) {
		const a = document.createElement('a');
		a.href = URL.createObjectURL(blob);
		a.download = name;
		a.click();
		setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
	}

	async function local(): Promise<boolean> {
		if (!db || !cascade) return false;
		let r = await localInput(db, cascade, choices);
		if ('missing' in r && navigator.onLine && (r.missing === 'rows' || r.missing === 'keys' || r.missing === 'distribution')) {
			// Materialise first: a cleared quiz's rows come from the questions and grades endpoints.
			const dm = downloadManager() ?? new DownloadManager(db);
			if (choices.scope === 'quiz') {
				const q = quizzes.find((x) => x.id === choices.quiz_id);
				if (q) await dm.materialiseQuiz(cascade, q, q.status === 'cleared').catch(() => undefined);
			}
			await ensureCascade(db, id).catch(() => undefined);
			r = await localInput(db, cascade, choices);
		}
		if (!('input' in r)) return false;
		const input = r.input;
		const type = choices.format === 'csv' ? 'text/csv' : 'text/plain';
		const worker = new Worker(new URL('../../../../lib/export/worker.ts', import.meta.url), { type: 'module' });
		const blob = await new Promise<Blob>((resolve) => {
			worker.onmessage = (ev) => resolve(ev.data as Blob);
			worker.postMessage({ input: $state.snapshot(input), type });
		});
		worker.terminate();
		save(blob, exportFilename(cascade.name, { ...choices, level: input.choices.level }));
		return true;
	}

	async function server() {
		if (!cascade) return;
		for (;;) {
			try {
				const { url } = await api.post<{ url: string }>(`/api/cascades/${id}/export-token`, serverChoices(cascade.quiz_type, choices));
				// A hidden frame: a non-attachment answer renders invisibly and is discarded.
				if (frame) frame.src = url;
				message = 'Your download has started.';
				return;
			} catch (e) {
				if (e instanceof ApiError && e.status === 429) {
					await new Promise((r) => setTimeout(r, (e.retryAfterSeconds ?? 5) * 1000));
					continue;
				}
				if (e instanceof ApiError && e.status === 404) {
					message = 'This has been deleted, on another device or by the Trash’s retention period.';
					setTimeout(() => goto('/cascades'), 2000);
					return;
				}
				if (e instanceof ApiError && e.status === 401) {
					message = 'Log in to export this.';
					return;
				}
				throw e;
			}
		}
	}

	async function exportNow() {
		if (!db || !cascade) return;
		busy = true;
		message = null;
		offerQuestions = false;
		try {
			// An export is an open (§ Downloads).
			const tx = db.transaction(['meta'], 'readwrite');
			await recordOpen(tx as unknown as RwTx, id, new Date().toISOString());
			await tx.done;
			if (await local()) return;
			if (!navigator.onLine) {
				message = 'This export needs a connection.';
				offerQuestions = choices.format === 'csv' || choices.lines === 'answers';
				return;
			}
			await server();
		} catch {
			message = 'The export could not be made. Check your connection.';
		} finally {
			busy = false;
		}
	}

	async function questionsAlone() {
		choices = { ...choices, format: 'txt', lines: 'questions' };
		await exportNow();
	}
</script>

<main class="mx-auto max-w-xl space-y-4 p-6">
	<h1 class="text-2xl font-semibold">Export {cascade?.name ?? ''}</h1>
	{#if cascade}
		<fieldset class="space-y-1">
			<legend class="font-medium">What</legend>
			<label class="flex items-center gap-2"><input type="radio" checked={choices.scope === 'cascade'} onchange={() => (choices = { ...choices, scope: 'cascade', quiz_id: undefined, order: 'study' })} /> The whole cascade</label>
			{#each quizzes as q (q.id)}
				<label class="flex items-center gap-2">
					<input type="radio" checked={choices.scope === 'quiz' && choices.quiz_id === q.id} onchange={() => (choices = { ...choices, scope: 'quiz', quiz_id: q.id })} />
					Level {q.level}{q.status === 'cleared' ? ' (in the Trash)' : ''} · {q.question_count}
				</label>
			{/each}
		</fieldset>
		<fieldset class="space-y-1">
			<legend class="font-medium">Which questions</legend>
			{#each WHICH as w (w.v)}
				<label class="flex items-center gap-2"><input type="radio" checked={choices.which === w.v} onchange={() => (choices = { ...choices, which: w.v })} /> {w.label}</label>
			{/each}
			{#if choices.scope === 'cascade'}
				<p class="text-sm text-muted-foreground">
					Across the cascade, a question is missed if it is missed in any level being studied, correct if it is correct in
					one and missed in none, and not yet answered otherwise.
				</p>
			{/if}
		</fieldset>
		<fieldset class="space-y-1">
			<legend class="font-medium">Format</legend>
			<label class="flex items-center gap-2"><input type="radio" checked={choices.format === 'txt'} onchange={() => (choices = { ...choices, format: 'txt' })} /> Word list (.txt)</label>
			<label class="flex items-center gap-2"><input type="radio" checked={choices.format === 'csv'} onchange={() => (choices = { ...choices, format: 'csv' })} /> Spreadsheet (.csv)</label>
		</fieldset>
		{#if choices.format === 'txt'}
			<fieldset class="space-y-1">
				<legend class="font-medium">Lines</legend>
				<label class="flex items-center gap-2"><input type="radio" checked={choices.lines === 'answers'} onchange={() => (choices = { ...choices, lines: 'answers' })} /> The answers</label>
				<label class="flex items-center gap-2"><input type="radio" checked={choices.lines === 'questions'} onchange={() => (choices = { ...choices, lines: 'questions' })} /> The questions</label>
			</fieldset>
		{:else}
			<fieldset class="space-y-1">
				<legend class="font-medium">Columns</legend>
				{#each columnsOffered as col (col)}
					<label class="flex items-center gap-2">
						<input
							type="checkbox"
							checked={choices.columns.includes(col)}
							onchange={(e) =>
								(choices = {
									...choices,
									columns: e.currentTarget.checked ? columnsOffered.filter((x) => x === col || choices.columns.includes(x)) : choices.columns.filter((x) => x !== col)
								})}
						/>
						{col}
					</label>
				{/each}
			</fieldset>
		{/if}
		{#if choices.scope === 'quiz'}
			<label class="flex items-center gap-2">
				<input type="checkbox" checked={choices.order === 'alphabetical'} onchange={(e) => (choices = { ...choices, order: e.currentTarget.checked ? 'alphabetical' : 'study' })} />
				Alphabetical instead of the quiz’s order
			</label>
		{/if}
		<p class="text-sm" role="status">
			{#if count !== null}{`${count} questions${entries !== null && choices.format === 'txt' && choices.lines === 'answers' ? ` · ${entries} words` : ''}`}{:else}Counting needs this cascade’s questions on this device.{/if}
		</p>
		<div class="flex gap-2">
			<Button onclick={exportNow} disabled={busy || (choices.format === 'csv' && choices.columns.length === 0)}>{busy ? 'Exporting…' : 'Export'}</Button>
			<Button variant="outline" onclick={() => history.back()}>Close</Button>
		</div>
		{#if message}<p class="text-sm" role="status">{message}</p>{/if}
		{#if offerQuestions}<Button variant="outline" onclick={questionsAlone}>Export the questions alone</Button>{/if}
	{:else}
		<p class="text-muted-foreground">This cascade is not on this device.</p>
	{/if}
	<iframe bind:this={frame} title="Download" class="hidden" hidden></iframe>
</main>
