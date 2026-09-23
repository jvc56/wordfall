<script lang="ts">
	import * as Alert from '$lib/components/ui/alert';
	let { message }: { message: string | null } = $props();
</script>

{#if message}
	<Alert.Root variant="destructive"><Alert.Description>{message}</Alert.Description></Alert.Root>
{/if}
