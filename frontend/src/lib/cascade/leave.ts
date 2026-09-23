// Leave value text (PLAN.md § Answers): n = trunc(|v| × 10^d + 0.5) in f64;
// n written as an integer with '.' d digits from the right; '-' when v < 0 and
// n > 0, '+' on screen when v > 0 and n > 0, no sign when n = 0, and never
// '+' in exports. No toFixed, which rounds half to even on the binary value.
export function leaveValueText(v: number, decimals: number, screen: boolean): string {
	const n = BigInt(Math.trunc(Math.abs(v) * 10 ** decimals + 0.5));
	let digits = n.toString();
	if (decimals > 0) {
		digits = digits.padStart(decimals + 1, '0');
		digits = `${digits.slice(0, -decimals)}.${digits.slice(-decimals)}`;
	}
	if (n > 0n && v < 0) return `-${digits}`;
	if (n > 0n && v > 0 && screen) return `+${digits}`;
	return digits;
}
