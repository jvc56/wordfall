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
	// default-src 'self' blocks data: URIs, so no asset is inlined as one.
	build: { assetsInlineLimit: 0 },
	server: {
		proxy: { '/api': process.env.WORDFALL_API ?? 'http://localhost:5173' }
	},
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
