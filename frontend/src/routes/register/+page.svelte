<script lang="ts">
	import { goto } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import AuthCard from '$lib/components/auth/AuthCard.svelte';
	import Field from '$lib/components/auth/Field.svelte';
	import FormError from '$lib/components/auth/FormError.svelte';
	import { ApiError, type FieldError } from '$lib/api';
	import { authApi } from '$lib/auth/client';
	import { describe } from '$lib/auth/errors';

	let username = $state('');
	let email = $state('');
	let password = $state('');
	let errors = $state<FieldError[]>([]);
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		busy = true;
		error = null;
		errors = [];
		try {
			await authApi.register(username, email, password);
			await goto(`/register/check-email?email=${encodeURIComponent(email)}`);
		} catch (err) {
			if (err instanceof ApiError && err.fieldErrors.length) errors = err.fieldErrors;
			else error = describe(err);
		} finally {
			busy = false;
		}
	}
</script>

<AuthCard title="Create an account">
	<form class="grid gap-4" onsubmit={submit}>
		<FormError message={error} />
		<Field id="username" label="Username" bind:value={username} {errors} autocomplete="username" />
		<Field id="email" label="Email" type="email" bind:value={email} {errors} autocomplete="email" />
		<Field id="password" label="Password" type="password" bind:value={password} {errors} autocomplete="new-password" />
		<p class="text-xs text-muted-foreground">A long phrase of unrelated words makes a strong password.</p>
		<Button type="submit" disabled={busy}>Create account</Button>
	</form>
	{#snippet footer()}
		<span>Already registered? <a class="underline" href="/login">Log in</a></span>
	{/snippet}
</AuthCard>
