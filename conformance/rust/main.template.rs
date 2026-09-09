// AUTO-GENERATED from main.template.rs by conformance/rust/gen-runner.ts — DO NOT EDIT
//
// Reads the conformance manifest, decodes every test case with the generated
// fixture types, and writes conformance/results/rust.json in the shared shape.

// @jst:mods

type Decoder = fn(&str) -> Result<(), String>;

// @jst:registrations

// @jst:excluded

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let manifest_path = &args[1];
    let out_path = &args[2];
    let t0 = std::time::Instant::now();

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let groups = manifest["groups"].as_array().expect("groups array");

    let decoder_map = decoders();
    let excluded_map = excluded();

    let mut total = 0u64;
    let mut pass = 0u64;
    let mut failures: Vec<serde_json::Value> = Vec::new();
    // keyword → (total, pass), insertion-ordered
    let mut keyword_order: Vec<String> = Vec::new();
    let mut by_keyword: std::collections::HashMap<String, (u64, u64)> =
        std::collections::HashMap::new();

    for g in groups {
        let id = g["id"].as_str().unwrap_or_default();
        let keyword = g["keyword"].as_str().unwrap_or_default();
        let type_name = g["type_name"].as_str().unwrap_or_default();
        let gen_error = g["errors"].get("rust").and_then(|e| e.as_str());
        let decoder = decoder_map.get(type_name);
        let skip_reason: Option<String> = match (gen_error, decoder) {
            (Some(e), _) => Some(format!("generation error: {e}")),
            (None, None) => Some(
                excluded_map
                    .get(type_name)
                    .cloned()
                    .unwrap_or_else(|| format!("no decoder for {type_name}")),
            ),
            _ => None,
        };
        if !by_keyword.contains_key(keyword) {
            keyword_order.push(keyword.to_string());
            by_keyword.insert(keyword.to_string(), (0, 0));
        }
        for t in g["tests"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
            let desc = t["description"].as_str().unwrap_or_default();
            let expected = t["valid"].as_bool().unwrap_or(false);
            total += 1;
            let kw = by_keyword.get_mut(keyword).expect("keyword entry");
            kw.0 += 1;
            let (actual, reason): (Option<bool>, Option<String>) = match (&skip_reason, decoder) {
                (None, Some(decode)) => {
                    let data = t["data"].to_string();
                    match decode(&data) {
                        Ok(()) => (Some(true), None),
                        Err(e) => (Some(false), Some(e)),
                    }
                }
                (skip, _) => (None, Some(skip.clone().unwrap_or_default())),
            };
            if actual == Some(expected) {
                pass += 1;
                kw.1 += 1;
            } else {
                failures.push(serde_json::json!({
                    "id": id,
                    "keyword": keyword,
                    "test": desc,
                    "expected": expected,
                    "actual": actual,
                    "reason": reason,
                }));
            }
        }
    }

    let by_keyword_json: serde_json::Map<String, serde_json::Value> = keyword_order
        .iter()
        .map(|k| {
            let (t, p) = by_keyword[k];
            (k.clone(), serde_json::json!({"total": t, "pass": p}))
        })
        .collect();
    let results = serde_json::json!({
        "language": "rust",
        "total": total,
        "pass": pass,
        "fail": total - pass,
        "failures": failures,
        "byKeyword": by_keyword_json,
    });
    std::fs::write(out_path, serde_json::to_string_pretty(&results).expect("serialize"))
        .expect("write results");

    let pct = if total > 0 {
        100.0 * pass as f64 / total as f64
    } else {
        0.0
    };
    println!(
        "rust conformance: {pass}/{total} passed ({pct:.1}%), {} failed",
        total - pass
    );
    println!("run time: {}ms", t0.elapsed().as_millis());

    let mut failing: Vec<(&String, u64, u64)> = keyword_order
        .iter()
        .map(|k| {
            let (t, p) = by_keyword[k];
            (k, t, t - p)
        })
        .filter(|(_, _, f)| *f > 0)
        .collect();
    failing.sort_by(|a, b| b.2.cmp(&a.2));
    if !failing.is_empty() {
        println!("top failing keywords:");
        for (k, t, f) in failing.iter().take(15) {
            println!("  {k}: {f}/{t} failing");
        }
    }
}
