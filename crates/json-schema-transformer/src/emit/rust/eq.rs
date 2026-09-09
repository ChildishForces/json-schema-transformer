/// Deep JSON-value equality per JSON Schema semantics: numbers compare
/// mathematically (1 == 1.0), big integers beyond double precision compare
/// exactly, booleans are never equal to numbers, strings never equal numbers.
pub fn _jst_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Null, serde_json::Value::Null) => true,
        (serde_json::Value::Bool(x), serde_json::Value::Bool(y)) => x == y,
        (serde_json::Value::String(x), serde_json::Value::String(y)) => x == y,
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
            // Exact integer comparison first (covers integers beyond f64 precision).
            if let (Some(i), Some(j)) = (x.as_i64(), y.as_i64()) {
                return i == j;
            }
            if let (Some(i), Some(j)) = (x.as_u64(), y.as_u64()) {
                return i == j;
            }
            // Mixed integer/float (and negative-vs-huge) fall back to mathematical
            // comparison via f64, matching 1 == 1.0.
            match (x.as_f64(), y.as_f64()) {
                (Some(f), Some(g)) => f == g,
                _ => false,
            }
        }
        (serde_json::Value::Array(x), serde_json::Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(u, v)| _jst_eq(u, v))
        }
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).map(|w| _jst_eq(v, w)).unwrap_or(false))
        }
        _ => false,
    }
}

/// Canonical string form of a JSON value, suitable for `uniqueItems`
/// de-duplication: object keys are sorted, numbers are canonicalized so that
/// 1 and 1.0 render identically while 1, true and "1" all differ. Agrees with
/// `_jst_eq` for values found in practice.
pub fn _jst_canonical(v: &serde_json::Value) -> String {
    let mut out = String::new();
    _jst_canonical_into(v, &mut out);
    out
}

fn _jst_canonical_into(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(f) = n.as_f64() {
                // Canonicalize floats with zero fractional part to their integer
                // rendering so 1.0 and 1 agree.
                if f.is_finite()
                    && f.fract() == 0.0
                    && (-9.223372036854776e18..1.8446744073709552e19).contains(&f)
                {
                    if f < 0.0 {
                        out.push_str(&(f as i64).to_string());
                    } else {
                        out.push_str(&(f as u64).to_string());
                    }
                } else {
                    out.push_str(&f.to_string());
                }
            } else {
                out.push_str("null");
            }
        }
        serde_json::Value::String(s) => {
            _jst_canonical_str(s, out);
        }
        serde_json::Value::Array(a) => {
            out.push('[');
            for (i, el) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                _jst_canonical_into(el, out);
            }
            out.push(']');
        }
        serde_json::Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                _jst_canonical_str(k, out);
                out.push(':');
                if let Some(val) = o.get(*k) {
                    _jst_canonical_into(val, out);
                }
            }
            out.push('}');
        }
    }
}

fn _jst_canonical_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
