<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Button } from '$lib/components/ui/button';
	import AuthCard from '$lib/components/auth/AuthCard.svelte';
	import Field from '$lib/components/auth/Field.svelte';
	import FormError from '$lib/components/auth/FormError.svelte';
	import { ApiError, type FieldError } from '$lib/api';
	import { authApi } from '$lib/auth/client';
	import { describe } from '$lib/auth/errors';

	let password = $state('');
	let errors = $state<FieldError[]>([]);
	let error = $state<string | null>(null);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = null;
		errors = [];
		try {
			await authApi.confirmReset(page.url.searchParams.get('token') ?? '', password);
			await goto('/login?reset=1');
		} catch (err) {
			if (err instanceof ApiError && err.fieldErrors.length) errors = err.fieldErrors;
			else if (err instanceof ApiError && err.status === 400)
				error = 'This reset link has expired or was already used. Ask for a new one.';
			else error = describe(err);
		}
	}
</script>

<AuthCard title="Set a new password" description="Every device will be signed out.">
	<form class="grid gap-4" onsubmit={submit}>
		<FormError message={error} />
		<Field id="password" label="New password" type="password" bind:value={password} {errors} autocomplete="new-password" />
		<Button type="submit">Set password</Button>
	</form>
</AuthCard>
