// Deterministic shuffles and question hashes (PLAN.md § Cascades →
// Deterministic shuffles). Bit for bit what the Rust module computes: every
// 64-bit value is a BigInt here, and decimal text on the wire.

const MASK = (1n << 64n) - 1n;
export const GAMMA = 0x9e3779b97f4a7c15n;
export const RESET_XOR = 0x9e3779b97f4a7c15n;

/** Vigna's reference splitmix64.c. */
export class SplitMix64 {
	private state: bigint;
	constructor(seed: bigint) {
		this.state = seed & MASK;
	}
	next(): bigint {
		this.state = (this.state + GAMMA) & MASK;
		let z = this.state;
		z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
		z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
		return z ^ (z >> 31n);
	}
}

/** The indexes sorted ascending, then Fisher–Yates from the last position
 * down to 1 with j = next() mod (i + 1). Returns the indexes in position order. */
export function shuffle(idx: number[], seed: bigint): number[] {
	const v = [...idx].sort((a, b) => a - b);
	const rng = new SplitMix64(seed);
	for (let i = v.length - 1; i >= 1; i--) {
		const j = Number(rng.next() % BigInt(i + 1));
		const t = v[i];
		v[i] = v[j];
		v[j] = t;
	}
	return v;
}

export function resetSeed(seed: bigint): bigint {
	return (seed ^ RESET_XOR) & MASK;
}

/**
 * FNV-1a 64 of the ascending indexes, each as a little-endian u32. Computed
 * on two 32-bit halves: the prime is 2^40 + 0x1b3, so h × prime is
 * (lo << 8 into the high half) + h × 0x1b3, every partial product exact in a double.
 */
export function questionsHash(idx: number[]): bigint {
	const v = [...idx].sort((a, b) => a - b);
	let hi = 0xcbf29ce4; // 14695981039346656037 = 0xcbf29ce484222325
	let lo = 0x84222325;
	for (const i of v) {
		for (let k = 0; k < 4; k++) {
			lo = (lo ^ ((i >>> (8 * k)) & 0xff)) >>> 0;
			const loProd = lo * 0x1b3;
			const carry = Math.floor(loProd / 0x100000000);
			const newLo = loProd % 0x100000000;
			hi =(hi * 0x1b3 + carry + ((lo * 256) % 0x100000000)) % 0x100000000;
			lo = newLo;
		}
	}
	return (BigInt(hi) << 32n) | BigInt(lo);
}

/** A u64 held as a stored two's-complement i64, and back. */
export function toI64(v: bigint): bigint {
	return BigInt.asIntN(64, v);
}
export function fromI64(v: bigint): bigint {
	return BigInt.asUintN(64, v);
}
