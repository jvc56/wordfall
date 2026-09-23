<script lang="ts">
	// Controls (PLAN.md § Controls): up to three bindings per action, captured
	// by pressing a key, clicking or scrolling inside a capture box; Escape
	// cancels and cannot be bound; a stroke moves from another action with a
	// notice; every action keeps one binding; Reset to defaults. Saved as one
	// `set_bindings` operation.
	import * as Card from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { applyLocally } from '$lib/local/apply';
	import type { UserDb } from '$lib/local/db';
	import { DEFAULT_BINDINGS } from '$lib/local/preferences';
	import type { Binding } from '$lib/local/rows';
	import { afterLocalWrite } from '$lib/sync/runtime.svelte';
	import { bind, keyStroke, mayNotReceive, mouseStroke, unbind, wheelStroke, type Action, type Stroke } from '$lib/player/bindings';

	let { db, bindings, onchange }: { db: UserDb; bindings: Binding[]; onchange: () => void } = $props();

	const ACTIONS: { action: Action; label: string }[] = [
		{ action: 'show_next', label: 'Show / Next' },
		{ action: 'toggle_grade', label: 'Toggle grade' },
		{ action: 'previous', label: 'Previous' }
	];

	let capturing = $state<Action | null>(null);
	let notice = $state<string | null>(null);
	let layout = $state<Map<string, string> | null>(null);

	$effect(() => {
		const kb = (navigator as unknown as { keyboard?: { getLayoutMap?: () => Promise<Map<string, string>> } }).keyboard;
		kb?.getLayoutMap?.().then((m) => (layout = m)).catch(() => undefined);
	});

	function label(b: Stroke): string {
		const mods = [b.ctrl && 'Ctrl', b.alt && 'Alt', b.shift && 'Shift', b.meta && 'Meta'].filter(Boolean).join('+');
		let base: string;
		if (b.kind === 'key') base = layout?.get(b.code)?.toUpperCase() ?? b.code.replace(/^Key/, '').replace(/^Digit/, '');
		else if (b.kind === 'wheel') base = `Wheel ${b.code}`;
		else base = `${b.code[0].toUpperCase()}${b.code.slice(1)} click`;
		return mods ? `${mods}+${base}` : base;
	}

	async function save(next: Binding[]) {
		await applyLocally(db, { type: 'set_bindings', bindings: next });
		afterLocalWrite();
		onchange();
	}

	async function captured(s: Stroke | null) {
		if (!capturing || !s) return;
		const r = bind(bindings, capturing, s);
		const action = capturing;
		capturing = null;
		if (r.refused) {
			notice = 'That binding can’t be added.';
			return;
		}
		notice = r.moved ? `${label(s)} moved from ${ACTIONS.find((a) => a.action === r.moved)!.label}.` : null;
		if (mayNotReceive(s)) notice = `${notice ?? ''} The browser or system may keep ${label(s)} for itself.`.trim();
		void action;
		await save(r.bindings);
	}

	function onKey(e: KeyboardEvent) {
		e.preventDefault();
		if (e.code === 'Escape') {
			capturing = null;
			return;
		}
		if (['ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight'].includes(e.code)) return;
		void captured(keyStroke(e));
	}
</script>

<Card.Root id="controls">
	<Card.Header><Card.Title>Controls</Card.Title></Card.Header>
	<Card.Content class="grid gap-4 text-sm">
		{#each ACTIONS as a (a.action)}
			<div>
				<p class="font-medium">{a.label}</p>
				<ul class="mt-1 flex flex-wrap gap-2">
					{#each bindings.filter((b) => b.action === a.action) as b, i (i)}
						<li class="flex items-center gap-1 rounded bg-muted px-2 py-1">
							{label(b)}
							<button aria-label="Remove binding" onclick={() => save(unbind(bindings, b))}>×</button>
						</li>
					{/each}
					{#if bindings.filter((b) => b.action === a.action).length < 3}
						<li><Button size="sm" variant="outline" onclick={() => (capturing = a.action)}>Add binding</Button></li>
					{/if}
				</ul>
			</div>
		{/each}
		{#if capturing}
			<!-- The capture box: press a key, click or scroll here; Escape cancels. -->
			<div
				class="rounded border-2 border-dashed p-6 text-center"
				role="button"
				tabindex="0"
				onkeydown={onKey}
				onmousedown={(e) => {
					e.preventDefault();
					void captured(mouseStroke(e));
				}}
				oncontextmenu={(e) => e.preventDefault()}
				onwheel={(e) => {
					e.preventDefault();
					void captured(wheelStroke(e));
				}}
				{@attach (el: HTMLElement) => el.focus()}
			>
				Press a key, click or scroll here. Escape cancels.
				{#if bindings.some((b) => b.kind === 'wheel')}
					<p class="text-xs text-muted-foreground">A bound wheel direction won’t scroll the quiz area; long answers still show a scrollbar.</p>
				{/if}
			</div>
		{/if}
		{#if notice}<p role="status">{notice}</p>{/if}
		<Button variant="outline" onclick={() => save(DEFAULT_BINDINGS)}>Reset to defaults</Button>
	</Card.Content>
</Card.Root>
