struct IPv4Address: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        let parts = value.split(separator: ".")
        guard parts.count == 4,
              parts.allSatisfy({ part in
                  guard let n = Int(part), (0...255).contains(n) else { return false }
                  return String(n) == part // reject leading zeros
              }) else {
            throw ValidationError.invalidIPv4(value)
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
