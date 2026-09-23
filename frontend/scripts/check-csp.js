// Fails the build unless build/index.html (the SPA fallback the service worker
// serves for every navigation) carries the <meta> CSP with 'self' and at least
// one hash in script-src and no 'unsafe-inline' there.
// PLAN.md § Deployment and Operations → Security headers.
import { readFileSync } from 'node:fs';

const file = process.argv[2] ?? 'build/index.html';
const html = readFileSync(file, 'utf8');
const fail = (msg) => {
	console.error(`CSP check failed for ${file}: ${msg}`);
	process.exit(1);
};

const meta = html.match(
	/<meta\s+http-equiv="content-security-policy"\s+content="([^"]*)"/i
);
if (!meta) fail('no <meta http-equiv="Content-Security-Policy"> policy');
const directives = Object.fromEntries(
	meta[1]
		.split(';')
		.map((d) => d.trim().split(/\s+/))
		.filter((parts) => parts[0])
		.map(([name, ...values]) => [name.toLowerCase(), values])
);
const scriptSrc = directives['script-src'];
if (!scriptSrc) fail('no script-src directive');
if (!scriptSrc.includes("'self'")) fail("script-src lacks 'self'");
if (!scriptSrc.some((v) => /^'sha(256|384|512)-/.test(v))) fail('script-src has no hash');
if (scriptSrc.includes("'unsafe-inline'")) fail("script-src allows 'unsafe-inline'");
console.log(`CSP check passed for ${file}`);
