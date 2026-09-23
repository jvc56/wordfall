// Client-side limits (PLAN.md § Configuration, last paragraph).

/** Every cascade opened or created here in this many days is kept downloaded. */
export const DOWNLOAD_WINDOW_DAYS = 14;
/** The soft limit on answer storage. */
export const ANSWER_STORAGE_SOFT_LIMIT_BYTES = 500 * 1024 * 1024;
/** Rows and question keys, measured. */
export const ROW_STORAGE_BUDGET = 2 * 1024 * 1024 * 1024;
/** A cascade this large is kept offline automatically once opened. */
export const AUTO_KEEP_OFFLINE_ROWS = 50_000;
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
