<script lang="ts">
	// Shared by the three admin uploads: upload progress, then either the
	// created item or the error list (the first 1,000, plus a total).
	import type { Snippet } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Progress } from '$lib/components/ui/progress';
	import * as Alert from '$lib/components/ui/alert';
	import { uploadFile, type UploadResult } from '$lib/admin/upload';

	let {
		path,
		accept,
		fields,
		onFile,
		children,
		created
	}: {
		path: string;
		accept: string;
		fields: () => Record<string, string>;
		onFile?: (file: File) => void;
		children?: Snippet;
		created: Snippet<[Record<string, unknown>]>;
	} = $props();

	let file = $state<File | null>(null);
	let progress = $state<number | null>(null);
	let result = $state<UploadResult | null>(null);

	function pick(e: Event) {
		const f = (e.currentTarget as HTMLInputElement).files?.[0] ?? null;
		file = f;
		result = null;
		if (f) onFile?.(f);
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!file) return;
		result = null;
		progress = 0;
		result = await uploadFile(path, fields(), file, (p) => (progress = p));
		progress = null;
	}
</script>

<form class="grid gap-4" onsubmit={submit}>
	{@render children?.()}
	<div class="grid gap-1.5">
		<Label for="file">File</Label>
		<Input id="file" type="file" {accept} onchange={pick} />
	</div>
	{#if progress !== null}
		<div class="grid gap-1">
			<Progress value={Math.round(progress * 100)} max={100} />
			<p class="text-xs text-muted-foreground">
				{progress < 1 ? `Uploading… ${Math.round(progress * 100)}%` : 'Validating and saving…'}
			</p>
		</div>
	{/if}
	<Button type="submit" disabled={!file || progress !== null}>Upload</Button>
</form>

{#if result?.ok}
	<Alert.Root class="mt-4">
		<Alert.Title>Uploaded</Alert.Title>
		<Alert.Description>{@render created(result.item)}</Alert.Description>
	</Alert.Root>
{:else if result}
	<Alert.Root variant="destructive" class="mt-4">
		<Alert.Title>
			{result.total === 1 ? '1 problem' : `${result.total.toLocaleString()} problems`}; nothing was
			stored
		</Alert.Title>
		<Alert.Description>
			<ul class="mt-2 max-h-96 overflow-auto font-mono text-xs">
				{#each result.errors as err}
					<li>{err.line != null ? `Line ${err.line}: ` : ''}{err.message}</li>
				{/each}
			</ul>
			{#if result.total > result.errors.length}
				<p class="mt-2">Showing the first {result.errors.length.toLocaleString()}.</p>
			{/if}
		</Alert.Description>
	</Alert.Root>
{/if}
