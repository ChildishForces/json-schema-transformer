struct IPv6Address: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        // Basic IPv6 validation: 1-8 groups of hex digits separated by ":"
        // Allows :: shorthand for consecutive zero groups
        let stripped = value.lowercased()
        let groups: [Substring]
        if stripped.contains("::") {
            let halves = stripped.split(separator: "::", maxSplits: 1, omittingEmptySubsequences: false)
            guard halves.count == 2 else { throw ValidationError.invalidIPv6(value) }
            let left = halves[0].isEmpty ? [] : halves[0].split(separator: ":")
            let right = halves[1].isEmpty ? [] : halves[1].split(separator: ":")
            guard left.count + right.count <= 7 else { throw ValidationError.invalidIPv6(value) }
            groups = left + right
        } else {
            groups = stripped.split(separator: ":")
            guard groups.count == 8 else { throw ValidationError.invalidIPv6(value) }
        }
        guard groups.allSatisfy({ g in
            (1...4).contains(g.count) && g.allSatisfy({ $0.isHexDigit })
        }) else {
            throw ValidationError.invalidIPv6(value)
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
