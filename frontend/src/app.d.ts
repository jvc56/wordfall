// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	/** The frontend build number (PLAN.md § API → POST /api/sync). */
	const __APP_BUILD__: number;
	const __APP_COMMIT__: string;
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
