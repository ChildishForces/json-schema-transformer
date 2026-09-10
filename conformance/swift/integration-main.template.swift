import Foundation

let cases: [(String, Bool)] = [
    (#"{"id":"a","quantity":2,"tags":["x","y"]}"#, true),
    (#"{"id":"a","quantity":1}"#, true),
    (#"{"id":"a","quantity":0}"#, false),
    (#"{"id":"a","quantity":1,"tags":["x","x"]}"#, false),
    (#"{"id":"a","quantity":1,"unknown":true}"#, false),
    (#"{"quantity":1}"#, false),
]
var ok = true
for (payload, expected) in cases {
    let data = payload.data(using: .utf8)!
    let actual = (try? JSONDecoder().decode(OrderItem.self, from: data)) != nil
    if actual != expected {
        ok = false
        print("MISMATCH: \(payload) expected=\(expected) actual=\(actual)")
    }
}

// Manual construction: the throwing initializer validates on the spot.
do {
    let item = try OrderItem(id: "a", quantity: 2)
    if try item.validatedJSONData().isEmpty {
        ok = false
        print("MISMATCH: validatedJSONData returned empty data")
    }
} catch {
    ok = false
    print("MISMATCH: valid manual construction threw \(error)")
}
if (try? OrderItem(id: "a", quantity: 0)) != nil {
    ok = false
    print("MISMATCH: OrderItem(id:\"a\", quantity:0) should throw")
}

print(ok ? "PASS" : "FAIL")
