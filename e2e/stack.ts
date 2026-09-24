// Calls into scripts/stack.py, the one definition of a working instance.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.resolve(here, '../scripts/stack.py');

/** Each configured pass of the suite runs its own stack (see the Makefile's test-e2e). */
export const PROJECT = process.env.E2E_PROJECT ?? 'wordfall-e2e';
export const PORT = Number(process.env.E2E_PORT ?? 5180);

export function stack(cmd: 'up' | 'seed' | 'reset' | 'down', args: string[] = []): string {
	return execFileSync(
		'python3',
		[script, cmd, '--project', PROJECT, '--port', String(PORT), ...args],
		{ encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }
	).trim();
}

/** Extra configuration for the suite's stack, through the same --env flag a developer uses. */
export function e2eEnv(): string[] {
	const extra = (process.env.E2E_ENV ?? '').split(/\s+/).filter(Boolean);
	return extra.flatMap((kv) => ['--env', kv]);
}
