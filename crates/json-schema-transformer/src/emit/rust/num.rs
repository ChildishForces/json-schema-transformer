/// JSON Schema "integer": any number with a zero fractional part (1.0 counts).
pub fn _jst_is_integer(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Number(n) => {
            n.is_i64()
                || n.is_u64()
                || n.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false)
        }
        _ => false,
    }
}

/// multipleOf check mirroring the float semantics used by the other emitters:
/// `(v / m)` must be finite and equal to its floor. Exact integer arithmetic is
/// used when both the value and divisor are integral and in range, so huge
/// integers are handled without float precision loss; otherwise falls back to
/// float division (0 or tiny divisors yield non-finite/fractional quotients and
/// fail, overflow to infinity fails).
pub fn _jst_multiple_of(v: &serde_json::Number, m: f64) -> bool {
    if m != 0.0 && m.is_finite() && m.fract() == 0.0 && m.abs() < 9.223372036854775e18 {
        let mi = m as i64;
        if let Some(x) = v.as_i64() {
            // checked_rem: i64::MIN % -1 overflows but is mathematically 0.
            return x.checked_rem(mi).map(|r| r == 0).unwrap_or(true);
        }
        if let Some(x) = v.as_u64() {
            return x % mi.unsigned_abs() == 0;
        }
    }
    match v.as_f64() {
        Some(x) => {
            let q = x / m;
            q.is_finite() && q == q.floor()
        }
        None => false,
    }
}
