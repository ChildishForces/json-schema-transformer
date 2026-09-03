struct HostnameString: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        guard value.count <= 253,
              !value.isEmpty,
              value.split(separator: ".").allSatisfy({
                  !$0.isEmpty && $0.count <= 63 &&
                  $0.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }) &&
                  !$0.hasPrefix("-") && !$0.hasSuffix("-")
              }) else {
            throw ValidationError.invalidHostname(value)
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
