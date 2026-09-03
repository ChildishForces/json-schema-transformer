private func _serializeAny(_ value: Any) -> Data {
    if JSONSerialization.isValidJSONObject(value) {
        return (try? JSONSerialization.data(withJSONObject: value, options: .sortedKeys)) ?? Data()
    }
    guard let wrapped = try? JSONSerialization.data(withJSONObject: [value], options: .sortedKeys),
          let str = String(data: wrapped, encoding: .utf8) else { return Data() }
    let inner = String(str.dropFirst().dropLast())
    return inner.data(using: .utf8) ?? Data()
}
