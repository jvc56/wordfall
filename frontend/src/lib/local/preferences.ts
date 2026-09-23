// The preferences view (PLAN.md § On the device → `preferences`): the base
// row as the server last sent it, with the fields of pending
// `set_preferences` operations laid over it in `device_seq` order and the
// latest pending `set_bindings` replacing the bindings, so a change made a
// moment ago cannot flip back before it is pushed. Until the first pull the
// documented defaults apply (§ Schema, `user_preferences`).
import type { UserDb } from './db';
import type { Binding, OutboxEntry, PreferencesRow } from './rows';

const binding = (action: Binding['action'], kind: Binding['kind'], code: string): Binding => ({
	action,
	kind,
	code,
	ctrl: false,
	shift: false,
	alt: false,
	meta: false
});

export const DEFAULT_BINDINGS: Binding[] = [
	binding('show_next', 'mouse_button', 'left'),
	binding('show_next', 'key', 'Space'),
	binding('toggle_grade', 'mouse_button', 'right'),
	binding('toggle_grade', 'key', 'KeyX'),
	binding('previous', 'mouse_button', 'middle'),
	binding('previous', 'key', 'Backspace')
];

export function defaultPreferences(): PreferencesRow {
	const epoch = new Date(0).toISOString();
	return {
		default_clear_threshold: 80,
		leave_value_decimals: 1,
		anagram_show_definitions: false,
		anagram_show_hooks: false,
		anagram_answer_mode: 'flashcard',
		default_segment_size: 0,
		default_progression: 'ladder',
		default_require_alphabetical: false,
		changed_at: epoch,
		changed_by_device_id: null,
		bindings_changed_at: epoch,
		bindings_device_id: null,
		updated_seq: 0,
		bindings: DEFAULT_BINDINGS
	};
}

const PREFERENCE_FIELDS = [
	'default_clear_threshold',
	'leave_value_decimals',
	'anagram_show_definitions',
	'anagram_show_hooks',
	'anagram_answer_mode',
	'default_segment_size',
	'default_progression',
	'default_require_alphabetical'
] as const;

/** The base row with the pending operations laid over it. */
export function layPreferences(base: PreferencesRow, pending: OutboxEntry[]): PreferencesRow {
	const view: PreferencesRow = { ...base, bindings: [...base.bindings] };
	for (const { op } of pending) {
		if (op.type === 'set_preferences') {
			for (const f of PREFERENCE_FIELDS) {
				if (op[f] !== undefined) (view as unknown as Record<string, unknown>)[f] = op[f];
			}
		} else if (op.type === 'set_bindings') {
			view.bindings = (op.bindings as Partial<Binding>[]).map((b) => ({
				...binding(b.action!, b.kind!, b.code!),
				...b
			}));
		}
	}
	return view;
}

export async function preferencesView(db: UserDb): Promise<PreferencesRow> {
	const tx = db.transaction(['preferences', 'outbox']);
	const base = (await tx.objectStore('preferences').get('row')) ?? defaultPreferences();
	const pending = await tx.objectStore('outbox').getAll();
	return layPreferences(base, pending);
}
