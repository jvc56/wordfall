import { stack, e2eEnv } from './stack';

export default async function globalSetup() {
	if (process.env.BASE_URL) return; // an existing instance, given an admin account
	stack('up', e2eEnv());
	stack('seed');
}
