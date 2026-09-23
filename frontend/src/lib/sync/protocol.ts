// The wire form of POST /api/sync (PLAN.md § API → Cascades and sync).
import type { Grade } from '$lib/cascade/rules';

export interface SyncResult {
	op_id: string;
	status: 'applied' | 'rejected';
	reason?: string;
	outcome?: string;
	new_quiz_question_count?: number;
	new_quiz_questions_hash?: string;
}

/** A quiz's graded rows in one page, with the lowest `updated_seq` among them. */
export interface QuestionGroup {
	quiz_id: string;
	question_idx: number[];
	grade: Grade[];
	graded_at: string[];
	min_updated_seq: string;
}

export interface Tombstone {
	entity: 'cascade' | 'quiz';
	entity_id: string;
	seq: string;
}

export type WireRow = Record<string, unknown>;

export interface Changes {
	cascades: WireRow[];
	quizzes: WireRow[];
	quiz_attempts: WireRow[];
	quiz_questions: QuestionGroup[];
	preferences?: WireRow;
	tombstones: Tombstone[];
}

export interface SyncResponse {
	results: SyncResult[];
	changes?: Changes;
	sync_seq: string;
	next_page_token?: string;
	resync_required?: boolean;
}

export interface SyncRequest {
	device_id: string;
	app_version: number;
	cursor: number | null;
	page_token?: string;
	question_rows_for: string[];
	ops?: unknown[];
}

/** One HTTP exchange: the status, the parsed body and any Retry-After. */
export interface Reply {
	status: number;
	body: unknown;
	retryAfter: number | null;
}

export type Transport = (req: SyncRequest) => Promise<Reply>;

/** Pushes are sent in batches of up to this many operations. */
export const BATCH_OPS = 500;
