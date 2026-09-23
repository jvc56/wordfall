import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// A static SPA: every navigation is served build/index.html (the fallback),
			// which is also what the service worker serves offline.
			adapter: adapter({ fallback: 'index.html', strict: false }),
			// PLAN.md § Deployment and Operations → Security headers. The build's hashes
			// allow exactly the inline bootstrap script; scripts/check-csp.js fails the
			// build if the policy does not reach build/index.html.
			csp: {
				mode: 'hash',
				directives: {
					'default-src': ['self'],
					'script-src': ['self'],
					'style-src': ['self', 'unsafe-inline'],
					'object-src': ['none']
				}
			}
		})
	],
	// PLAN.md § API → POST /api/sync: the build's monotonic integer build number,
	// compared against MIN_APP_VERSION, and its git hash, logged but deciding nothing.
	define: {
		__APP_BUILD__: JSON.stringify(Number(process.env.WORDFALL_BUILD ?? 0)),
		__APP_COMMIT__: JSON.stringify(process.env.WORDFALL_COMMIT ?? 'dev')
	},
	// default-src 'self' blocks data: URIs, so no asset is inlined as one.
	build: { assetsInlineLimit: 0 },
	server: {
		proxy: { '/api': process.env.WORDFALL_API ?? 'http://localhost:5173' }
	},
	// Component tests mount Svelte's browser build.
	resolve: process.env.VITEST ? { conditions: ['browser'] } : undefined,
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
