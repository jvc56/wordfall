// Deleting a set of rows as key ranges (PLAN.md § On the device). Chromium's
// IndexedDB takes about as long to delete one row by key as to write a
// hundred, so a cascade's 300,000 rows deleted one by one cost minutes;
// deleted as a range they go in one request. Every row a range covers belongs
// to the set, because each store's key starts with what the set is keyed by.

// The idb wrappers' store types differ per store; these take any of them.
type Store = { delete(key: IDBKeyRange | IDBValidKey): Promise<unknown>; index(name: string): { getAllKeys(query: IDBValidKey): Promise<IDBValidKey[]> } };

/** Every row whose key is `[prefix, n]`. */
export function prefixRange(prefix: string): IDBKeyRange {
	return IDBKeyRange.bound([prefix, -Infinity], [prefix, Infinity]);
}

/** A cascade's rows in a store keyed `[cascade_id, idx]` (`questions`, `cards`). */
export async function deleteCascadeKeyed(store: unknown, cascadeId: string) {
	await (store as Store).delete(prefixRange(cascadeId));
}

/** A cascade's rows in a store keyed `[quiz_id, n]` with a `cascade_id` index: one range per quiz. */
export async function deleteQuizKeyed(store: unknown, cascadeId: string) {
	const s = store as Store;
	const quizzes = new Set(((await s.index('cascade_id').getAllKeys(cascadeId)) as [string, number][]).map((k) => k[0]));
	await Promise.all([...quizzes].map((q) => s.delete(prefixRange(q))));
}

/** A store's rows whose keys the `cascade_id` index lists, key by key (stores keyed by id: few rows). */
export async function deleteListed(store: unknown, cascadeId: string) {
	const s = store as Store;
	await Promise.all((await s.index('cascade_id').getAllKeys(cascadeId)).map((k) => s.delete(k)));
}
