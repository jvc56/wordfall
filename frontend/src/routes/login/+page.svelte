<script lang="ts">
	import { goto } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import AuthCard from '$lib/components/auth/AuthCard.svelte';
	import Field from '$lib/components/auth/Field.svelte';
	import FormError from '$lib/components/auth/FormError.svelte';
	import { ApiError } from '$lib/api';
	import { login } from '$lib/auth/session.svelte';
	import { describe } from '$lib/auth/errors';

	let username = $state('');
	let password = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = null;
		try {
			await login(username, password);
			await goto('/cascades');
		} catch (err) {
			if (err instanceof ApiError && err.status === 401) error = 'Wrong username or password.';
			else if (err instanceof ApiError && err.status === 403)
				error = 'Confirm your email address first: follow the link we sent you.';
			else error = describe(err);
		} finally {
			busy = false;
		}
	}
</script>

<AuthCard title="Log in" description="Log in with your username.">
	<form class="grid gap-4" onsubmit={submit}>
		<FormError message={error} />
		<Field id="username" label="Username" bind:value={username} autocomplete="username" />
		<Field id="password" label="Password" type="password" bind:value={password} autocomplete="current-password" />
		<Button type="submit" disabled={busy}>Log in</Button>
	</form>
	{#snippet footer()}
		<div class="flex w-full justify-between">
			<a class="underline" href="/register">Create an account</a>
			<a class="underline" href="/reset-password">Forgot password?</a>
		</div>
	{/snippet}
</AuthCard>
