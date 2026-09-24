<script lang="ts">
	// Catalog overview with delete actions (PLAN.md § Admin). Admins only;
	// the API answers 404 to everyone else, and so does this page.
	import { onMount } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Table from '$lib/components/ui/table';
	import { api, ApiError } from '$lib/api';
	import NotFound from '$lib/components/NotFound.svelte';

	interface Item {
		id: number;
		name?: string;
		lexicon?: string;
		letter_distribution?: string;
		uploaded_by: string | null;
		uploaded_at: string;
		loading: boolean;
		loaded_by: string[];
		tile_count?: number;
		word_count?: number;
		leave_count?: number;
		lexicon_count?: number;
		cascade_count?: number;
		has_leave_set?: boolean;
		index_bytes?: number | null;
		build_ms?: number | null;
	}
	interface Catalog {
		instance_id: string;
		instances: { instance_id: string; heartbeat_at: string }[];
		letter_distributions: Item[];
		lexicons: Item[];
		leave_sets: Item[];
	}

	let catalog = $state<Catalog | null>(null);
	let notFound = $state(false);
	let error = $state<string | null>(null);

	async function load() {
		try {
			catalog = await api.get<Catalog>('/api/admin/catalog');
		} catch (e) {
			if (e instanceof ApiError && e.status === 404) notFound = true;
			else error = 'Could not load the catalog.';
		}
	}

	onMount(load);

	async function remove(kind: string, id: number, label: string) {
		if (!confirm(`Delete ${label}? This cannot be undone.`)) return;
		error = null;
		try {
			await api.delete(`/api/admin/${kind}/${id}`);
		} catch (e) {
			error = e instanceof ApiError && e.status === 409 ? `${label} is still in use.` : 'Delete failed.';
		}
		await load();
	}

	const when = (s: string) => new Date(s).toLocaleString();
	const mb = (b: number | null | undefined) => (b == null ? '—' : `${(b / 1e6).toFixed(1)} MB`);

	function lexiconBlocker(l: Item): string | null {
		const parts = [];
		if (l.has_leave_set) parts.push('it has leave values');
		if (l.cascade_count) parts.push(l.cascade_count === 1 ? '1 cascade uses it' : `${l.cascade_count} cascades use it`);
		return parts.length ? `In use: ${parts.join(' and ')}.` : null;
	}
</script>

{#if notFound}
	<NotFound />
{:else}
	<main class="mx-auto grid max-w-6xl gap-6 p-6">
		<header class="flex flex-wrap items-center justify-between gap-3">
			<h1 class="text-2xl font-semibold">Catalog</h1>
			<div class="flex flex-wrap gap-2">
				<Button href="/admin/letter-distributions/new" variant="outline">Upload distribution</Button>
				<Button href="/admin/lexicons/new" variant="outline">Upload lexicon</Button>
				<Button href="/admin/leave-sets/new" variant="outline">Upload leave values</Button>
			</div>
		</header>
		{#if error}<p class="text-sm text-destructive">{error}</p>{/if}
		{#if catalog}
			<p class="text-sm text-muted-foreground">
				{catalog.instances.length} live instance{catalog.instances.length === 1 ? '' : 's'}. An
				item marked <em>loading</em> is not yet indexed by every instance, and is not offered in
				the cascade builder until it is.
			</p>

			<Card.Root>
				<Card.Header><Card.Title>Letter distributions</Card.Title></Card.Header>
				<Card.Content>
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Name</Table.Head><Table.Head>Tiles</Table.Head>
								<Table.Head>Uploaded</Table.Head><Table.Head>Lexicons</Table.Head>
								<Table.Head></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each catalog.letter_distributions as d (d.id)}
								<Table.Row>
									<Table.Cell>
										{d.name}
										{#if d.loading}<Badge variant="secondary">loading</Badge>{/if}
									</Table.Cell>
									<Table.Cell>{d.tile_count}</Table.Cell>
									<Table.Cell>{when(d.uploaded_at)} by {d.uploaded_by ?? 'a deleted user'}</Table.Cell>
									<Table.Cell>{d.lexicon_count}</Table.Cell>
									<Table.Cell class="text-right">
										<Button
											size="sm"
											variant="destructive"
											disabled={!!d.lexicon_count}
											title={d.lexicon_count ? 'In use: lexicons refer to it.' : undefined}
											onclick={() => remove('letter-distributions', d.id, d.name ?? '')}
										>
											Delete
										</Button>
										{#if d.lexicon_count}<p class="mt-1 text-xs text-muted-foreground">In use: lexicons refer to it.</p>{/if}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header><Card.Title>Lexicons</Card.Title></Card.Header>
				<Card.Content>
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Name</Table.Head><Table.Head>Distribution</Table.Head>
								<Table.Head>Words</Table.Head><Table.Head>Uploaded</Table.Head>
								<Table.Head>Cascades</Table.Head><Table.Head>Index</Table.Head>
								<Table.Head></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each catalog.lexicons as l (l.id)}
								{@const blocker = lexiconBlocker(l)}
								<Table.Row>
									<Table.Cell>
										{l.name}
										{#if l.loading}<Badge variant="secondary">loading</Badge>{/if}
									</Table.Cell>
									<Table.Cell>{l.letter_distribution}</Table.Cell>
									<Table.Cell>{l.word_count?.toLocaleString()}</Table.Cell>
									<Table.Cell>{when(l.uploaded_at)} by {l.uploaded_by ?? 'a deleted user'}</Table.Cell>
									<Table.Cell>{l.cascade_count}</Table.Cell>
									<Table.Cell>{mb(l.index_bytes)}{l.build_ms != null ? `, ${l.build_ms} ms` : ''}</Table.Cell>
									<Table.Cell class="text-right">
										<Button
											size="sm"
											variant="destructive"
											disabled={!!blocker}
											title={blocker ?? undefined}
											onclick={() => remove('lexicons', l.id, l.name ?? '')}
										>
											Delete
										</Button>
										{#if blocker}<p class="mt-1 text-xs text-muted-foreground">{blocker}</p>{/if}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header><Card.Title>Leave values</Card.Title></Card.Header>
				<Card.Content>
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Lexicon</Table.Head><Table.Head>Leaves</Table.Head>
								<Table.Head>Uploaded</Table.Head><Table.Head>Cascades</Table.Head>
								<Table.Head>Index</Table.Head><Table.Head></Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each catalog.leave_sets as s (s.id)}
								<Table.Row>
									<Table.Cell>
										{s.lexicon}
										{#if s.loading}<Badge variant="secondary">loading</Badge>{/if}
									</Table.Cell>
									<Table.Cell>{s.leave_count?.toLocaleString()}</Table.Cell>
									<Table.Cell>{when(s.uploaded_at)} by {s.uploaded_by ?? 'a deleted user'}</Table.Cell>
									<Table.Cell>{s.cascade_count}</Table.Cell>
									<Table.Cell>{mb(s.index_bytes)}{s.build_ms != null ? `, ${s.build_ms} ms` : ''}</Table.Cell>
									<Table.Cell class="text-right">
										<Button
											size="sm"
											variant="destructive"
											disabled={!!s.cascade_count}
											title={s.cascade_count ? 'In use: Leave Value cascades refer to it.' : undefined}
											onclick={() => remove('leave-sets', s.id, `the leave values of ${s.lexicon}`)}
										>
											Delete
										</Button>
										{#if s.cascade_count}<p class="mt-1 text-xs text-muted-foreground">In use: Leave Value cascades refer to it.</p>{/if}
									</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header><Card.Title>Instances</Card.Title></Card.Header>
				<Card.Content>
					<ul class="text-sm">
						{#each catalog.instances as i (i.instance_id)}
							<li>
								<code>{i.instance_id}</code>{i.instance_id === catalog.instance_id ? ' (this one)' : ''}
								— heartbeat {when(i.heartbeat_at)}
							</li>
						{/each}
					</ul>
				</Card.Content>
			</Card.Root>
		{/if}
	</main>
{/if}
