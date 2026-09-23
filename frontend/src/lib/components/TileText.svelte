<script lang="ts">
	// Renders MAGPIE notation, drawing a multi-character tile as one joined
	// tile without brackets (PLAN.md § Frontend → TileText). With `lower`, a
	// tile is lower-cased only where it is a single ASCII letter, so a
	// multi-character or non-ASCII tile is never shown in its blank form.
	let { text, lower = false, class: klass = '' }: { text: string; lower?: boolean; class?: string } = $props();

	interface Piece {
		t: string;
		multi: boolean;
	}

	let pieces = $derived.by((): Piece[] => {
		const out: Piece[] = [];
		const cs = Array.from(text);
		for (let i = 0; i < cs.length; i++) {
			if (cs[i] === '[') {
				const end = cs.indexOf(']', i + 1);
				if (end > i) {
					out.push({ t: cs.slice(i + 1, end).join(''), multi: true });
					i = end;
					continue;
				}
			}
			const c = cs[i];
			out.push({ t: lower && /^[A-Z]$/.test(c) ? c.toLowerCase() : c, multi: false });
		}
		return out;
	});
</script>

<span class={klass}
	>{#each pieces as p, i (i)}{#if p.multi}<span
				class="mx-px inline-block rounded-sm bg-muted px-0.5 leading-tight tracking-tighter">{p.t}</span
			>{:else}{p.t}{/if}{/each}</span
>
