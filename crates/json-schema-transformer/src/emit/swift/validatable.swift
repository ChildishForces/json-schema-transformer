protocol Validatable: Codable {
    func validate() throws
}

extension Validatable {
    /// Round-trips through the validating decoder — a matched default
    /// JSONEncoder/JSONDecoder pair, so Date and friends stay consistent.
    func validate() throws {
        let data = try JSONEncoder().encode(self)
        _ = try JSONDecoder().decode(Self.self, from: data)
    }

    var isValid: Bool { (try? validate()) != nil }

    /// Validate, then encode. Plain `JSONEncoder().encode(_:)` does NOT
    /// validate (validate() itself encodes — hooking encode would recurse).
    func validatedJSONData() throws -> Data {
        try validate()
        return try JSONEncoder().encode(self)
    }
}
