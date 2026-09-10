import Foundation

var ok = true
let data = #"{"id":"a","quantity":2}"#.data(using: .utf8)!
guard var item = try? JSONDecoder().decode(OrderItem.self, from: data) else {
    print("MISMATCH: valid payload failed to decode")
    print("FAIL")
    exit(1)
}
if !item.isValid {
    ok = false
    print("MISMATCH: freshly decoded item should be valid")
}

// Mutate into an invalid state: validation APIs must notice.
item.quantity = 0
if item.isValid {
    ok = false
    print("MISMATCH: quantity=0 should be invalid")
}
if (try? item.validatedJSONData()) != nil {
    ok = false
    print("MISMATCH: validatedJSONData should throw for invalid item")
}

// Mutate back to a valid state: validate() must pass again.
item.quantity = 3
do {
    try item.validate()
} catch {
    ok = false
    print("MISMATCH: validate() after fixing quantity threw \(error)")
}

print(ok ? "PASS" : "FAIL")
