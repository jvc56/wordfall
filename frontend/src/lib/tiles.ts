// Tiles (PLAN.md § Tiles). A tile is its index in the distribution — its line
// number from 0, so 0 is the blank and index order is tile order. Alphabetical
// order everywhere is the lexicographic order of tile-index sequences, a prefix
// sorting first.

export const BLANK = 0;

export interface TileDef {
	letter: string;
	blank_letter: string;
	count: number;
	value: number;
	is_vowel: boolean;
}

export class TileError extends Error {}

/** Code points, so a letter like `L·L` or `Ç` counts as it does in Rust. */
function chars(s: string): string[] {
	return Array.from(s);
}

export class Distribution {
	readonly byLetter = new Map<string, number>();
	private readonly byBlank = new Map<string, number>();
	/** Letters longest first, for greedy matching. */
	private readonly letters: { chars: string[]; tile: number }[] = [];

	constructor(
		readonly name: string,
		readonly tiles: TileDef[]
	) {
		tiles.forEach((t, i) => {
			this.byLetter.set(t.letter, i);
			if (i !== BLANK) {
				this.byBlank.set(t.blank_letter, i);
				this.letters.push({ chars: chars(t.letter), tile: i });
			}
		});
		this.letters.sort((a, b) => b.chars.length - a.chars.length || a.tile - b.tile);
	}

	get maxValue(): number {
		return Math.max(0, ...this.tiles.map((t) => t.value));
	}

	/** True when every tile but the blank is a plain A–Z letter. */
	get plainAZ(): boolean {
		return this.tiles.slice(1).every((t) => /^[A-Z]$/.test(t.letter));
	}

	letter(t: number): string {
		return this.tiles[t].letter;
	}

	isMulti(t: number): boolean {
		return chars(this.tiles[t].letter).length > 1;
	}

	/** MAGPIE notation: multi-character tiles in brackets. */
	toMagpie(tiles: number[]): string {
		return tiles.map((t) => (this.isMulti(t) ? `[${this.letter(t)}]` : this.letter(t))).join('');
	}

	/** Display form: letters without brackets. */
	toDisplay(tiles: number[]): string {
		return tiles.map((t) => this.letter(t)).join('');
	}

	private lookup(s: string, allowBlank: boolean): number {
		if (s === '?') {
			if (!allowBlank) throw new TileError('use `.` for any single tile');
			return BLANK;
		}
		const t = this.byLetter.get(s);
		if (t !== undefined && t !== BLANK) return t;
		if (this.byBlank.has(s)) throw new TileError(`'${s}' is a lower-case tile, which means a blank`);
		throw new TileError(`'${s}' is not a tile of this distribution`);
	}

	/** Strict MAGPIE notation, as the server parses it. */
	parseMagpie(s: string, allowBlank: boolean): number[] {
		const cs = chars(s);
		const out: number[] = [];
		for (let i = 0; i < cs.length; i++) {
			const c = cs[i];
			if (c === '[') {
				const end = cs.indexOf(']', i + 1);
				if (end < 0) throw new TileError("a '[' is never closed");
				const inner = cs.slice(i + 1, end);
				if (inner.includes('[')) throw new TileError('brackets cannot be nested');
				if (inner.length === 0) throw new TileError("empty brackets '[]'");
				if (inner.length === 1) throw new TileError('single-character tiles are written without brackets');
				out.push(this.lookup(inner.join(''), allowBlank));
				i = end;
			} else if (c === ']') {
				throw new TileError("a ']' has no matching '['");
			} else {
				const t = this.lookup(c, allowBlank);
				if (this.isMulti(t)) throw new TileError(`'${c}' is not a tile of this distribution`);
				out.push(t);
			}
		}
		if (out.length === 0) throw new TileError('no tiles');
		return out;
	}

	/** Greedy longest-first matching of one space-free piece, with no backtracking. */
	private greedy(piece: string[], allowBlank: boolean, out: number[]) {
		let i = 0;
		outer: while (i < piece.length) {
			if (piece[i] === '?') {
				out.push(this.lookup('?', allowBlank));
				i++;
				continue;
			}
			for (const { chars: l, tile } of this.letters) {
				if (l.every((ch, k) => piece[i + k] === ch)) {
					out.push(tile);
					i += l.length;
					continue outer;
				}
			}
			throw new TileError(`'${piece.slice(i).join('')}' is not a tile of this distribution`);
		}
	}

	/**
	 * Typed text to tiles: upper-case it, treat a bracketed group as one
	 * multi-character tile, split the rest at spaces and match greedily.
	 */
	parseTyped(s: string, allowBlank: boolean): number[] {
		const cs = chars(s.toUpperCase());
		const out: number[] = [];
		let piece: string[] = [];
		for (let i = 0; i < cs.length; i++) {
			const c = cs[i];
			if (c === '[') {
				this.greedy(piece, allowBlank, out);
				piece = [];
				const end = cs.indexOf(']', i + 1);
				if (end < 0) throw new TileError("a '[' is never closed");
				const inner = cs.slice(i + 1, end);
				if (inner.includes('[')) throw new TileError('brackets cannot be nested');
				if (inner.length === 0) throw new TileError("empty brackets '[]'");
				out.push(this.lookup(inner.join(''), allowBlank));
				i = end;
			} else if (c === ']') {
				throw new TileError("a ']' has no matching '['");
			} else if (/\s/.test(c)) {
				this.greedy(piece, allowBlank, out);
				piece = [];
			} else {
				piece.push(c);
			}
		}
		this.greedy(piece, allowBlank, out);
		if (out.length === 0) throw new TileError('no tiles');
		return out;
	}

	/** A typed tile list in canonical MAGPIE notation. */
	canonicalTiles(typed: string, allowBlank: boolean): string {
		return this.toMagpie(this.parseTyped(typed, allowBlank));
	}

	/** A leave in canonical order: tile order, blank first. */
	canonicalLeave(typed: string): string {
		return this.toMagpie(canonicalLeaveOrder(this.parseTyped(typed, true)));
	}

	/**
	 * A typed pattern in canonical form: tokens separated by single spaces.
	 * Brackets always mean a set; inside and out, pieces are matched greedily,
	 * and a space separates tiles.
	 */
	canonicalPattern(typed: string, allowBlank: boolean): string {
		const cs = chars(typed.toUpperCase());
		const toks: string[] = [];
		const flush = (piece: string[]) => {
			let buf: string[] = [];
			const emit = () => {
				if (buf.length) {
					const tiles: number[] = [];
					this.greedy(buf, allowBlank, tiles);
					for (const t of tiles) toks.push(t === BLANK ? '?' : this.letter(t));
				}
				buf = [];
			};
			for (const c of piece) {
				if (c === '.' || c === '*') {
					emit();
					toks.push(c);
				} else buf.push(c);
			}
			emit();
		};
		let piece: string[] = [];
		for (let i = 0; i < cs.length; i++) {
			const c = cs[i];
			if (c === '[') {
				flush(piece);
				piece = [];
				const end = cs.indexOf(']', i + 1);
				if (end < 0) throw new TileError('unbalanced bracket');
				const inner = cs.slice(i + 1, end);
				if (inner.includes('[')) throw new TileError('unbalanced bracket');
				const tiles: number[] = [];
				for (const part of inner.join('').split(/\s+/).filter(Boolean)) {
					this.greedy(chars(part), allowBlank, tiles);
				}
				if (tiles.length === 0) throw new TileError('empty brackets');
				toks.push(`[${tiles.map((t) => (t === BLANK ? '?' : this.letter(t))).join(' ')}]`);
				i = end;
			} else if (c === ']') {
				throw new TileError('unbalanced bracket');
			} else if (/\s/.test(c)) {
				flush(piece);
				piece = [];
			} else {
				piece.push(c);
			}
		}
		flush(piece);
		if (toks.length === 0) throw new TileError('the pattern is empty');
		return toks.join(' ');
	}
}

export function canonicalLeaveOrder(tiles: number[]): number[] {
	return [...tiles].sort((a, b) => a - b);
}

/** Tile order: lexicographic over indexes, a prefix first. */
export function compareTiles(a: number[], b: number[]): number {
	const n = Math.min(a.length, b.length);
	for (let i = 0; i < n; i++) if (a[i] !== b[i]) return a[i] - b[i];
	return a.length - b.length;
}

/** Whether MAGPIE notation text is well formed, without a distribution. */
export function magpieSyntaxError(s: string, allowBlank: boolean): string | null {
	const cs = chars(s);
	if (cs.length === 0) return 'enter at least one tile';
	for (let i = 0; i < cs.length; i++) {
		const c = cs[i];
		if (c === '[') {
			const end = cs.indexOf(']', i + 1);
			if (end < 0) return "a '[' is never closed";
			const inner = cs.slice(i + 1, end);
			if (inner.includes('[')) return 'brackets cannot be nested';
			if (inner.length === 0) return "empty brackets '[]'";
			if (inner.length === 1) return 'single-character tiles are written without brackets';
			i = end;
		} else if (c === ']') return "a ']' has no matching '['";
		else if (c === '?' && !allowBlank) return 'use `.` for any single tile';
		else if (/\s/.test(c)) return 'tiles are written without spaces';
	}
	return null;
}
