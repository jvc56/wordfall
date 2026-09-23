// Controls (PLAN.md § Taking a quiz → Controls, Typed mode → Protecting
// typing). Strokes are keys by physical position (`KeyboardEvent.code`),
// mouse buttons or wheel directions, each with an exact set of modifiers.
import type { Binding } from '$lib/local/rows';

export type Action = Binding['action'];
export type Stroke = Omit<Binding, 'action'>;

const MOUSE = ['left', 'middle', 'right', 'back', 'forward'];

const mods = (e: { ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }) => ({
	ctrl: e.ctrlKey,
	shift: e.shiftKey,
	alt: e.altKey,
	meta: e.metaKey
});

export function keyStroke(e: KeyboardEvent): Stroke {
	return { kind: 'key', code: e.code, ...mods(e) };
}

export function mouseStroke(e: MouseEvent): Stroke | null {
	const code = MOUSE[e.button];
	return code ? { kind: 'mouse_button', code, ...mods(e) } : null;
}

export function wheelStroke(e: WheelEvent): Stroke | null {
	if (e.deltaY === 0) return null;
	return { kind: 'wheel', code: e.deltaY < 0 ? 'up' : 'down', ...mods(e) };
}

export function sameStroke(a: Stroke, b: Stroke): boolean {
	return a.kind === b.kind && a.code === b.code && a.ctrl === b.ctrl && a.shift === b.shift && a.alt === b.alt && a.meta === b.meta;
}

export function actionFor(bindings: Binding[], s: Stroke): Action | null {
	return bindings.find((b) => sameStroke(b, s))?.action ?? null;
}

/**
 * While the typed input has focus, every stroke that types, edits or submits
 * goes to the input: any key producing a character with or without Shift,
 * Space, Backspace, Delete and Enter. Strokes with Ctrl, Alt or Meta still
 * act, and so do Escape, the arrow keys and function keys.
 */
export function inputOwns(e: KeyboardEvent): boolean {
	if (e.ctrlKey || e.altKey || e.metaKey) return false;
	if (['Space', 'Backspace', 'Delete', 'Enter', 'NumpadEnter'].includes(e.code)) return true;
	return e.key.length === 1;
}

/** Drops the same action fired twice within `ms` (120 ms for actions, 150 ms for the wheel). */
export class Debounce {
	private last = new Map<string, number>();
	constructor(private readonly ms: number) {}
	pass(key: string, now: number): boolean {
		const t = this.last.get(key);
		if (t !== undefined && now - t < this.ms) return false;
		this.last.set(key, now);
		return true;
	}
}

/** The strokes a page may never receive, which the capture box warns about. */
export function mayNotReceive(s: Stroke): boolean {
	if (s.kind === 'mouse_button') return s.code === 'back' || s.code === 'forward';
	if (s.kind !== 'key') return false;
	return (s.ctrl && ['KeyW', 'KeyT', 'KeyN', 'Tab'].includes(s.code)) || (s.meta && ['KeyQ', 'KeyW'].includes(s.code));
}

/**
 * Binding a stroke to an action: a stroke belongs to one action, so it moves
 * from any other (`moved` names that one); at most three per action.
 */
export function bind(bindings: Binding[], action: Action, s: Stroke): { bindings: Binding[]; moved: Action | null; refused: boolean } {
	if (s.kind === 'key' && s.code === 'Escape') return { bindings, moved: null, refused: true };
	const existing = bindings.find((b) => sameStroke(b, s));
	if (existing?.action === action) return { bindings, moved: null, refused: false };
	const rest = bindings.filter((b) => !sameStroke(b, s));
	if (existing && rest.filter((b) => b.action === existing.action).length === 0) {
		// Every action must keep at least one binding.
		return { bindings, moved: null, refused: true };
	}
	if (rest.filter((b) => b.action === action).length >= 3) return { bindings, moved: null, refused: true };
	return { bindings: [...rest, { action, ...s }], moved: existing ? existing.action : null, refused: false };
}

export function unbind(bindings: Binding[], target: Binding): Binding[] {
	const rest = bindings.filter((b) => b !== target);
	return rest.some((b) => b.action === target.action) ? rest : bindings;
}
