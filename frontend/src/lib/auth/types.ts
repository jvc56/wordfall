/** `GET /api/auth/me` (PLAN.md § API → Auth and account). */
export interface Me {
	user_id: string;
	username: string;
	is_admin: boolean;
	trash_retention_days: number;
	max_quiz_questions: number;
}
