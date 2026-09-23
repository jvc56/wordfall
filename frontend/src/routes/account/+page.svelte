<script lang="ts">
	import { goto } from '$app/navigation';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Label } from '$lib/components/ui/label';
	import Field from '$lib/components/auth/Field.svelte';
	import FormError from '$lib/components/auth/FormError.svelte';
	import { ApiError, type FieldError } from '$lib/api';
	import { authApi } from '$lib/auth/client';
	import { describe } from '$lib/auth/errors';
	import { forgetSession, logout, session } from '$lib/auth/session.svelte';
	import { removeAccountData } from '$lib/local/accounts';

	let current = $state('');
	let next = $state('');
	let pwErrors = $state<FieldError[]>([]);
	let pwMessage = $state<string | null>(null);
	let pwError = $state<string | null>(null);

	let everywherePw = $state('');
	let everywhereErrors = $state<FieldError[]>([]);
	let everywhereMessage = $state<string | null>(null);

	let deletePw = $state('');
	let deleteErrors = $state<FieldError[]>([]);
	let deleteError = $state<string | null>(null);

	let removeData = $state(false);

	function fieldErrorsOr(e: unknown, set: (f: FieldError[]) => void): string | null {
		if (e instanceof ApiError && e.fieldErrors.length) {
			set(e.fieldErrors);
			return null;
		}
		return describe(e);
	}

	async function changePassword(e: SubmitEvent) {
		e.preventDefault();
		pwErrors = [];
		pwError = pwMessage = null;
		try {
			await authApi.changePassword(current, next);
			current = next = '';
			pwMessage = 'Password changed. Your other devices have been signed out.';
		} catch (err) {
			pwError = fieldErrorsOr(err, (f) => (pwErrors = f));
		}
	}

	async function signOutEverywhere(e: SubmitEvent) {
		e.preventDefault();
		everywhereErrors = [];
		everywhereMessage = null;
		try {
			await authApi.signOutEverywhere(everywherePw);
			everywherePw = '';
			everywhereMessage = 'Every other session has been signed out. This one keeps working.';
		} catch (err) {
			everywhereMessage = fieldErrorsOr(err, (f) => (everywhereErrors = f));
		}
	}

	async function deleteAccount(e: SubmitEvent) {
		e.preventDefault();
		deleteErrors = [];
		deleteError = null;
		const id = session.userId;
		try {
			await authApi.deleteAccount(deletePw);
		} catch (err) {
			deleteError = fieldErrorsOr(err, (f) => (deleteErrors = f));
			return;
		}
		// The session is already void: remove this device's copy, with no queued logout.
		if (id) await removeAccountData(id);
		forgetSession();
		await goto('/', { replaceState: true });
	}

	async function doLogout() {
		const id = session.userId;
		await logout();
		if (removeData && id) await removeAccountData(id);
		await goto('/login', { replaceState: true });
	}

	// Map the account endpoints' field names onto the inputs here.
	const rename = (errs: FieldError[], from: string, to: string) =>
		errs.map((e) => (e.field === from ? { ...e, field: to } : e));
</script>

<main class="mx-auto grid max-w-2xl gap-6 p-6">
	<header class="flex items-center justify-between">
		<h1 class="text-2xl font-semibold">Account</h1>
		<Button href="/cascades" variant="outline">Cascades</Button>
	</header>

	<Card.Root>
		<Card.Header><Card.Title>Signed in as {session.username}</Card.Title></Card.Header>
		<Card.Content class="grid gap-3">
			<div class="flex items-center gap-2">
				<Checkbox id="remove-data" bind:checked={removeData} />
				<Label for="remove-data">Remove this account's data from this device</Label>
			</div>
			<Button variant="outline" onclick={doLogout}>Log out</Button>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header><Card.Title>Change password</Card.Title></Card.Header>
		<Card.Content>
			<form class="grid gap-4" onsubmit={changePassword}>
				<FormError message={pwError} />
				{#if pwMessage}<p class="text-sm">{pwMessage}</p>{/if}
				<Field id="current_password" label="Current password" type="password" bind:value={current} errors={pwErrors} autocomplete="current-password" />
				<Field id="new_password" label="New password" type="password" bind:value={next} errors={pwErrors} autocomplete="new-password" />
				<Button type="submit">Change password</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title>Sign out everywhere</Card.Title>
			<Card.Description>Signs out every other session. This device stays signed in.</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="grid gap-4" onsubmit={signOutEverywhere}>
				{#if everywhereMessage}<p class="text-sm">{everywhereMessage}</p>{/if}
				<Field id="everywhere_password" label="Password" type="password" bind:value={everywherePw} errors={rename(everywhereErrors, 'password', 'everywhere_password')} autocomplete="current-password" />
				<Button type="submit" variant="outline">Sign out everywhere</Button>
			</form>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title>Delete account</Card.Title>
			<Card.Description>Deletes your account and everything in it, for good.</Card.Description>
		</Card.Header>
		<Card.Content>
			<form class="grid gap-4" onsubmit={deleteAccount}>
				<FormError message={deleteError} />
				<Field id="delete_password" label="Password" type="password" bind:value={deletePw} errors={rename(deleteErrors, 'password', 'delete_password')} autocomplete="current-password" />
				<Button type="submit" variant="destructive">Delete account</Button>
			</form>
		</Card.Content>
	</Card.Root>
</main>
