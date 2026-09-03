struct IPAddress: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        // Try IPv4 first, then IPv6
        if (try? IPv4Address(value)) != nil || (try? IPv6Address(value)) != nil {
            self.value = value
        } else {
            throw ValidationError.invalidIP(value)
        }
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
