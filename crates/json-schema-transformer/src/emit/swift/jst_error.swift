/// A single validation failure: a JSON Pointer (RFC 6901) to the offending
/// location ("" = the whole value) and a human-readable message.
private struct JstIssue {
    let path: String
    let message: String
}

/// Validation error carrying every failed constraint, not just the first.
/// `errorDescription` renders all issues, "; "-separated, each as
/// `path: message` (root-level issues render the message alone).
private struct JstValidationError: Error, LocalizedError {
    let issues: [JstIssue]

    var errorDescription: String? {
        issues.map { $0.path.isEmpty ? $0.message : "\($0.path): \($0.message)" }.joined(separator: "; ")
    }
}

/// Escape a key for use as a JSON Pointer token (RFC 6901: `~` → `~0`, `/` → `~1`).
private func _jstPtr(_ segment: String) -> String {
    segment.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1")
}

/// Render a Codable coding path as a JSON Pointer.
private func _jstPointer(_ codingPath: [CodingKey]) -> String {
    codingPath.map { key in "/" + (key.intValue.map(String.init) ?? _jstPtr(key.stringValue)) }.joined()
}

/// Convert an error thrown while decoding/validating a sub-value into issues
/// anchored at `path`. `containerBase` is the enclosing decoder's codingPath
/// depth when the failed decode ran inside the same decode session (the
/// DecodingError context path is then rebased to the current type); fresh
/// JSONDecoder sub-decodes leave it nil (context paths are already relative
/// to the sub-document, so `path` is prefixed).
private func _jstIssues(_ error: Error, at path: String, containerBase: Int? = nil) -> [JstIssue] {
    if let e = error as? JstValidationError {
        return e.issues.map { JstIssue(path: path + $0.path, message: $0.message) }
    }
    if let e = error as? DecodingError {
        let prefix = containerBase == nil ? path : ""
        func rebase(_ ctx: DecodingError.Context) -> String {
            prefix + _jstPointer(Array(ctx.codingPath.dropFirst(containerBase ?? 0)))
        }
        switch e {
        case .keyNotFound(let key, let ctx):
            return [JstIssue(path: rebase(ctx) + "/" + _jstPtr(key.stringValue), message: "required property is missing")]
        case .typeMismatch(let type, let ctx):
            return [JstIssue(path: rebase(ctx), message: "expected \(type)")]
        case .valueNotFound(let type, let ctx):
            return [JstIssue(path: rebase(ctx), message: "expected \(type), found null")]
        case .dataCorrupted(let ctx):
            return [JstIssue(path: rebase(ctx), message: ctx.debugDescription.isEmpty ? "invalid value" : ctx.debugDescription)]
        @unknown default:
            return [JstIssue(path: path, message: "invalid value")]
        }
    }
    if let e = error as? LocalizedError, let d = e.errorDescription {
        return [JstIssue(path: path, message: d)]
    }
    return [JstIssue(path: path, message: "invalid value")]
}
