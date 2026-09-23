// Admin uploads (PLAN.md § Admin → Uploads): multipart, with upload progress,
// the session's CSRF token and the tab's account binding.
import { boundUser, readCookie, CSRF_COOKIE, CSRF_HEADER, USER_HEADER } from '$lib/api';

export interface LineError {
	line: number | null;
	message: string;
}

export type UploadResult =
	| { ok: true; item: Record<string, unknown> }
	| { ok: false; status: number; errors: LineError[]; total: number; retryAfter: number | null };

export function uploadFile(
	path: string,
	fields: Record<string, string>,
	file: File,
	onProgress: (fraction: number) => void
): Promise<UploadResult> {
	return new Promise((resolve) => {
		const form = new FormData();
		for (const [k, v] of Object.entries(fields)) form.append(k, v);
		form.append('file', file, file.name);
		const xhr = new XMLHttpRequest();
		xhr.open('POST', path);
		const csrf = readCookie(CSRF_COOKIE);
		if (csrf) xhr.setRequestHeader(CSRF_HEADER, csrf);
		const user = boundUser();
		if (user) xhr.setRequestHeader(USER_HEADER, user);
		xhr.upload.onprogress = (e) => {
			if (e.lengthComputable) onProgress(e.loaded / e.total);
		};
		xhr.onload = () => {
			let body: Record<string, unknown> = {};
			try {
				body = JSON.parse(xhr.responseText);
			} catch {
				/* not JSON */
			}
			if (xhr.status === 201) {
				resolve({ ok: true, item: body });
				return;
			}
			const errors = Array.isArray(body.errors)
				? (body.errors as LineError[])
				: [{ line: null, message: messageFor(xhr.status) }];
			const ra = Number(xhr.getResponseHeader('Retry-After'));
			resolve({
				ok: false,
				status: xhr.status,
				errors,
				total: typeof body.total_errors === 'number' ? body.total_errors : errors.length,
				retryAfter: Number.isFinite(ra) && ra > 0 ? ra : null
			});
		};
		xhr.onerror = () =>
			resolve({
				ok: false,
				status: 0,
				errors: [{ line: null, message: 'Could not reach Wordfall.' }],
				total: 1,
				retryAfter: null
			});
		xhr.send(form);
	});
}

function messageFor(status: number): string {
	if (status === 429) return 'Too many uploads. Wait a minute and try again.';
	if (status === 413) return 'The file is larger than 100 MB.';
	if (status === 404) return 'Not found.';
	if (status === 408) return 'The upload took longer than 120 seconds.';
	return `The upload failed (HTTP ${status}).`;
}
