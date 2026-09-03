struct Base64String: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        guard Data(base64Encoded: value) != nil else {
            throw ValidationError.invalidBase64(value)
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
