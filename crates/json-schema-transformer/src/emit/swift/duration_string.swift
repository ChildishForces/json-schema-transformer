struct DurationString: Codable, Hashable, CustomStringConvertible {
    let value: String

    var description: String { value }

    init(_ value: String) throws {
        // ISO 8601 duration: P[nY][nM][nD][T[nH][nM][nS]]
        let pattern = try! Regex(#"P(\d+Y)?(\d+M)?(\d+D)?(T(\d+H)?(\d+M)?(\d+(\.\d+)?S)?)?"#)
        guard value.wholeMatch(of: pattern) != nil, value != "P" else {
            throw ValidationError.invalidDuration(value)
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
