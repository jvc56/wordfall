// Calls for the auth and account endpoints.
import { api } from '$lib/api';
import type { Me } from './types';

export const authApi = {
	register: (username: string, email: string, password: string) =>
		api.post<unknown>('/api/auth/register', { username, email, password }, { bindUser: false }),
	confirmEmail: (code: string) =>
		api.post<unknown>('/api/auth/confirm-email', { code }, { bindUser: false }),
	login: (username: string, password: string) =>
		api.post<Me>('/api/auth/login', { username, password }, { bindUser: false }),
	/** Exempt from the account binding; sends the CSRF token when a cookie exists. */
	logout: () => api.post<unknown>('/api/auth/logout', undefined, { bindUser: false }),
	me: () => api.get<Me>('/api/auth/me', { bindUser: false }),
	requestReset: (email: string) =>
		api.post<unknown>('/api/auth/reset-password', { email }, { bindUser: false }),
	confirmReset: (token: string, password: string) =>
		api.post<unknown>('/api/auth/reset-password/confirm', { token, password }, { bindUser: false }),
	signOutEverywhere: (password: string) =>
		api.post<unknown>('/api/account/sign-out-everywhere', { password }),
	changePassword: (current_password: string, new_password: string) =>
		api.post<unknown>('/api/account/password', { current_password, new_password }),
	deleteAccount: (password: string) => api.delete<unknown>('/api/account', { password })
};
