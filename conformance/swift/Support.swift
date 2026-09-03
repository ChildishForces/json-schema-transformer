// Shared runtime support for the Swift conformance harness.
// Generated fixture files reference `AnyCodable` (internal, single definition
// for the whole module) — all other shared helpers are file-private in the
// generated files themselves.

import Foundation

struct AnyCodable: Codable {
    let value: Any

    init(_ value: Any) { self.value = value }

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            value = NSNull()
        } else if let b = try? c.decode(Bool.self) {
            value = b
        } else if let i = try? c.decode(Int.self) {
            value = i
        } else if let d = try? c.decode(Double.self) {
            value = d
        } else if let s = try? c.decode(String.self) {
            value = s
        } else if let a = try? c.decode([AnyCodable].self) {
            value = a.map { $0.value }
        } else if let o = try? c.decode([String: AnyCodable].self) {
            value = o.mapValues { $0.value }
        } else {
            throw DecodingError.dataCorruptedError(in: c, debugDescription: "unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch value {
        case is NSNull:
            try c.encodeNil()
        case let b as Bool:
            try c.encode(b)
        case let i as Int:
            try c.encode(i)
        case let d as Double:
            try c.encode(d)
        case let s as String:
            try c.encode(s)
        case let a as [Any]:
            try c.encode(a.map(AnyCodable.init))
        case let o as [String: Any]:
            try c.encode(o.mapValues(AnyCodable.init))
        default:
            throw EncodingError.invalidValue(
                value,
                .init(codingPath: c.codingPath, debugDescription: "unsupported value")
            )
        }
    }
}
