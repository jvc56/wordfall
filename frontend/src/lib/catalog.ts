// Catalog reads for the builder: GET /api/lexicons and the distribution.
import { api } from '$lib/api';
import { sendWithRetry } from '$lib/builder/create';
import type { LexiconInfo } from '$lib/filters';
import { Distribution, type TileDef } from '$lib/tiles';

export async function loadLexicons(): Promise<LexiconInfo[]> {
	return sendWithRetry(() => api.get<LexiconInfo[]>('/api/lexicons', { bindUser: false }), { maxAttempts: 5 });
}

const cache = new Map<string, Promise<Distribution>>();

export function loadDistribution(name: string): Promise<Distribution> {
	let p = cache.get(name);
	if (!p) {
		p = sendWithRetry(
			() => api.get<{ name: string; tiles: TileDef[] }>(`/api/letter-distributions/${encodeURIComponent(name)}`, { bindUser: false }),
			{ maxAttempts: 5 }
		).then((d) => new Distribution(d.name, d.tiles));
		p.catch(() => cache.delete(name));
		cache.set(name, p);
	}
	return p;
}
