struct TimeString: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        // RFC 3339 time: HH:MM:SS[.fractional][Z|+HH:MM|-HH:MM]
        let pattern = try! Regex(#"(?i)\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?"#)
        guard value.wholeMatch(of: pattern) != nil else {
            throw ValidationError.invalidTime(value)
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
