struct EmailAddress: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        let pattern = try! Regex(#"[^@\s]+@[^@\s]+\.[^@\s]+"#)
        guard value.wholeMatch(of: pattern) != nil else {
            throw ValidationError.invalidEmail(value)
        }
        self.value = value
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        try self.init(container.decode(String.self))
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(value)
    }
}
