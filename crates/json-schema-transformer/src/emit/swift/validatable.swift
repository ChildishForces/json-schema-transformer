protocol Validatable: Codable {
    func validate() throws
}

extension Validatable {
    /// Round-trips through the validating decoder — a matched ISO-8601
    /// JSONEncoder/JSONDecoder pair, so `Date` fields survive the trip and
    /// serialize as schema-valid RFC 3339 strings.
    func validate() throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        _ = try decoder.decode(Self.self, from: encoder.encode(self))
    }

    var isValid: Bool { (try? validate()) != nil }

    /// Validate, then encode. Plain `JSONEncoder().encode(_:)` does NOT
    /// validate (validate() itself encodes — hooking encode would recurse).
    func validatedJSONData() throws -> Data {
        try validate()
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return try encoder.encode(self)
    }
}
