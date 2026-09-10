mod jst_helpers;
mod order_item;

use order_item::OrderItem;

fn main() {
    let cases: &[(&str, bool)] = &[
        (r#"{"id":"a","quantity":2,"tags":["x","y"]}"#, true),
        (r#"{"id":"a","quantity":1}"#, true),
        (r#"{"id":"a","quantity":0}"#, false),
        (r#"{"id":"a","quantity":1,"tags":["x","x"]}"#, false),
        (r#"{"id":"a","quantity":1,"unknown":true}"#, false),
        (r#"{"quantity":1}"#, false),
    ];
    let mut ok = true;
    for (payload, expected) in cases {
        let actual = serde_json::from_str::<OrderItem>(payload).is_ok();
        if actual != *expected {
            ok = false;
            println!("MISMATCH: {payload} expected={expected} actual={actual}");
        }
    }

    // Direct construction (fields are pub), mutation, and re-validation.
    let mut item = OrderItem {
        id: "a".into(),
        quantity: serde_json::Number::from(2),
        tags: None,
    };
    if item.validate().is_err() {
        ok = false;
        println!("MISMATCH: constructed valid item failed validate()");
    }
    item.quantity = serde_json::Number::from(0);
    if item.validate().is_ok() {
        ok = false;
        println!("MISMATCH: quantity=0 passed validate()");
    }
    if item.to_validated_json().is_ok() {
        ok = false;
        println!("MISMATCH: quantity=0 passed to_validated_json()");
    }
    item.quantity = serde_json::Number::from(2);
    if item.to_validated_json().is_err() {
        ok = false;
        println!("MISMATCH: restored item failed to_validated_json()");
    }

    println!("{}", if ok { "PASS" } else { "FAIL" });
}
