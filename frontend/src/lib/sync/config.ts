// Client-side limits (PLAN.md § Configuration, last paragraph).

/**
 * Storage limits lowered for a test build: the end-to-end budget journeys'
 * stack builds its frontend with WORDFALL_TEST_LIMITS (PLAN.md § End-to-end
 * tests: "Over ROW_STORAGE_BUDGET, lowered with a test constant"). A release
 * build never sets it, and these are the plan's values.
 */
const TEST: Partial<Record<'ANSWER_STORAGE_SOFT_LIMIT_BYTES' | 'ROW_STORAGE_BUDGET' | 'AUTO_KEEP_OFFLINE_ROWS', number>> =
	(typeof __TEST_LIMITS__ !== 'undefined' && __TEST_LIMITS__) || {};

/** Every cascade opened or created here in this many days is kept downloaded. */
export const DOWNLOAD_WINDOW_DAYS = 14;
/** The soft limit on answer storage. */
export const ANSWER_STORAGE_SOFT_LIMIT_BYTES = TEST.ANSWER_STORAGE_SOFT_LIMIT_BYTES ?? 500 * 1024 * 1024;
/** Rows and question keys, measured. */
export const ROW_STORAGE_BUDGET = TEST.ROW_STORAGE_BUDGET ?? 2 * 1024 * 1024 * 1024;
/** A cascade this large is kept offline automatically once opened. */
export const AUTO_KEEP_OFFLINE_ROWS = TEST.AUTO_KEEP_OFFLINE_ROWS ?? 50_000;
export const SYNC_INTERVAL_MS = 30_000;
export const PREVIEW_DEBOUNCE_MS = 400;
export const PREVIEW_MIN_INTERVAL_MS = 2_000;
/**
 * The server's SEARCH_RATE_PER_MINUTE default, which the preview's reserve is
 * measured against (PQ-010: the client is not told the configured value).
 */
export const SEARCH_RATE_PER_MINUTE = 30;
/** Slots the preview leaves for Create Cascade, Start over and Save Search…. */
export const PREVIEW_RESERVE = 2;
/** Above this many In Word List entries the preview runs only on its button. */
export const PREVIEW_MANUAL_ENTRIES = 10_000;
