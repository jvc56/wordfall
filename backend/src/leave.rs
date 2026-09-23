//! Leave value text (PLAN.md § Answers): half away from zero on the f64,
//! `n = trunc(|v| × 10^d + 0.5)`, written as an integer with a `.` inserted
//! `d` digits from the right. `-` when `v < 0` and `n > 0`; `+` on screen when
//! `v > 0` and `n > 0`; no sign when `n = 0`; never `+` in exports. No
//! `format!("{:.d}")`, which rounds half to even on the binary value.

pub fn leave_value_text(v: f64, decimals: u32, screen: bool) -> String {
    let n = (v.abs() * 10f64.powi(decimals as i32) + 0.5).trunc() as u64;
    let mut digits = n.to_string();
    if decimals > 0 {
        let d = decimals as usize;
        while digits.len() < d + 1 {
            digits.insert(0, '0');
        }
        digits.insert(digits.len() - d, '.');
    }
    if n > 0 && v < 0.0 {
        format!("-{digits}")
    } else if n > 0 && v > 0.0 && screen {
        format!("+{digits}")
    } else {
        digits
    }
}
