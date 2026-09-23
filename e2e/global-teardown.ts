import { stack } from './stack';

export default async function globalTeardown() {
	if (process.env.BASE_URL || process.env.E2E_KEEP_STACK) return;
	stack('down', process.env.CI ? ['--volumes'] : []);
}
