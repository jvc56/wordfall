// The per-user database (PLAN.md § On the device → IndexedDB stores): one per
// user id, opened only for the signed-in account, upgraded by versioned
// migrations. A tab still on an old build closes its connection on
// `versionchange`, so the new build's upgrade is never blocked.
import { openDB, type DBSchema, type IDBPDatabase, type StoreNames } from 'idb';
import { userDbName } from './accounts';
import type {
	AttemptRow,
	CardRow,
	CascadeRow,
	DistributionRow,
	OutboxEntry,
	PreferencesRow,
	QuestionKey,
	QuestionRow,
	QuizRow
} from './rows';

type QuestionKeyPath = [string, number];

interface RowStores {
	cascades: { key: string; value: CascadeRow };
	quizzes: { key: string; value: QuizRow; indexes: { cascade_id: string } };
	quiz_questions: {
		key: QuestionKeyPath;
		value: QuestionRow;
		indexes: { quiz_id: string; cascade_id: string; quiz_position: [string, number] };
	};
	quiz_attempts: { key: QuestionKeyPath; value: AttemptRow; indexes: { quiz_id: string; cascade_id: string } };
}

export interface UserSchema extends DBSchema {
	meta: { key: string; value: unknown };
	preferences: { key: string; value: PreferencesRow };
	distributions: { key: string; value: DistributionRow };
	cascades: RowStores['cascades'];
	quizzes: RowStores['quizzes'];
	quiz_questions: RowStores['quiz_questions'];
	quiz_attempts: RowStores['quiz_attempts'];
	overlay_cascades: RowStores['cascades'];
	overlay_quizzes: RowStores['quizzes'];
	overlay_quiz_questions: RowStores['quiz_questions'];
	overlay_quiz_attempts: RowStores['quiz_attempts'];
	staging_cascades: RowStores['cascades'];
	staging_quizzes: RowStores['quizzes'];
	staging_quiz_questions: RowStores['quiz_questions'];
	staging_quiz_attempts: RowStores['quiz_attempts'];
	questions: { key: QuestionKeyPath; value: QuestionKey; indexes: { cascade_id: string } };
	cards: { key: QuestionKeyPath; value: CardRow; indexes: { cascade_id: string } };
	outbox: { key: number; value: OutboxEntry; indexes: { cascade_id: string; cursor_quiz: string } };
}

export type UserDb = IDBPDatabase<UserSchema>;
export type StoreName = StoreNames<UserSchema>;

export const BASE = ['cascades', 'quizzes', 'quiz_questions', 'quiz_attempts'] as const;
export const OVERLAY = ['overlay_cascades', 'overlay_quizzes', 'overlay_quiz_questions', 'overlay_quiz_attempts'] as const;
export const STAGING = ['staging_cascades', 'staging_quizzes', 'staging_quiz_questions', 'staging_quiz_attempts'] as const;

function rowStores(db: IDBPDatabase<UserSchema>, [c, q, qq, qa]: readonly [string, string, string, string]) {
	// The generic names defeat idb's typing here; the shapes are RowStores'.
	const d = db as unknown as IDBPDatabase;
	d.createObjectStore(c, { keyPath: 'id' });
	const quizzes = d.createObjectStore(q, { keyPath: 'id' });
	quizzes.createIndex('cascade_id', 'cascade_id');
	const questions = d.createObjectStore(qq, { keyPath: ['quiz_id', 'question_idx'] });
	questions.createIndex('quiz_id', 'quiz_id');
	questions.createIndex('cascade_id', 'cascade_id');
	questions.createIndex('quiz_position', ['quiz_id', 'position']);
	const attempts = d.createObjectStore(qa, { keyPath: ['quiz_id', 'attempt'] });
	attempts.createIndex('quiz_id', 'quiz_id');
	attempts.createIndex('cascade_id', 'cascade_id');
}

/** Versioned migrations; index i upgrades from version i to i + 1. */
export const MIGRATIONS: Array<(db: IDBPDatabase<UserSchema>) => void> = [
	(db) => {
		db.createObjectStore('meta');
		db.createObjectStore('preferences');
		db.createObjectStore('distributions', { keyPath: 'name' });
		rowStores(db, BASE);
		rowStores(db, OVERLAY);
		rowStores(db, STAGING);
		db.createObjectStore('questions', { keyPath: ['cascade_id', 'idx'] }).createIndex('cascade_id', 'cascade_id');
		db.createObjectStore('cards', { keyPath: ['cascade_id', 'idx'] }).createIndex('cascade_id', 'cascade_id');
		const outbox = db.createObjectStore('outbox', { keyPath: 'device_seq' });
		outbox.createIndex('cascade_id', 'cascade_id');
		outbox.createIndex('cursor_quiz', 'cursor_quiz');
	}
];
export const USER_DB_VERSION = MIGRATIONS.length;

/**
 * Opens the signed-in account's database. `onVersionChange` runs when a newer
 * build in another tab needs the database: the connection is already closed
 * by then, and the caller shows "Wordfall was updated in another tab — reload
 * to continue".
 */
export async function openUserDb(userId: string, onVersionChange?: () => void): Promise<UserDb> {
	const db = await openDB<UserSchema>(userDbName(userId), USER_DB_VERSION, {
		upgrade(db, oldVersion) {
			for (let v = oldVersion; v < USER_DB_VERSION; v++) MIGRATIONS[v](db);
		}
	});
	db.addEventListener('versionchange', () => {
		db.close();
		onVersionChange?.();
	});
	return db;
}
