// JSON value with bool/number distinction and reference identity (class) so
// schema nodes can be indexed by object identity for base-URI tracking.
private final class JSIVal: Codable {
    enum Kind {
        case null
        case bool(Bool)
        case num(Double)
        case str(String)
        case arr([JSIVal])
        case obj([String: JSIVal])
    }
    let kind: Kind

    init(_ kind: Kind) { self.kind = kind }

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            kind = .null
        } else if let b = try? c.decode(Bool.self) {
            kind = .bool(b)
        } else if let i = try? c.decode(Int.self) {
            kind = .num(Double(i))
        } else if let d = try? c.decode(Double.self) {
            kind = .num(d)
        } else if let s = try? c.decode(String.self) {
            kind = .str(s)
        } else if let a = try? c.decode([JSIVal].self) {
            kind = .arr(a)
        } else if let o = try? c.decode([String: JSIVal].self) {
            kind = .obj(o)
        } else {
            throw DecodingError.dataCorruptedError(in: c, debugDescription: "unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch kind {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .num(let d):
            if d.isFinite && d.rounded() == d && abs(d) < 9e15 {
                try c.encode(Int64(d))
            } else {
                try c.encode(d)
            }
        case .str(let s): try c.encode(s)
        case .arr(let a): try c.encode(a)
        case .obj(let o): try c.encode(o)
        }
    }
}

/// JSON Schema equality: numbers compare by value; strings by exact code
/// points (NOT Swift's canonical-equivalence ==).
private func jsiEqual(_ a: JSIVal, _ b: JSIVal) -> Bool {
    switch (a.kind, b.kind) {
    case (.null, .null): return true
    case (.bool(let x), .bool(let y)): return x == y
    case (.num(let x), .num(let y)): return x == y
    case (.str(let x), .str(let y)): return x.unicodeScalars.elementsEqual(y.unicodeScalars)
    case (.arr(let x), .arr(let y)):
        return x.count == y.count && zip(x, y).allSatisfy { jsiEqual($0, $1) }
    case (.obj(let x), .obj(let y)):
        if x.count != y.count { return false }
        for (k, v) in x {
            guard let w = y[k], jsiEqual(v, w) else { return false }
        }
        return true
    default: return false
    }
}

private func jsiStripFrag(_ u: String) -> String {
    if let i = u.firstIndex(of: "#") { return String(u[..<i]) }
    return u
}

private func jsiResolveUri(_ base: String, _ ref: String) -> String {
    if ref.hasPrefix("#") { return jsiStripFrag(base) + ref }
    if ref.isEmpty { return jsiStripFrag(base) }
    if let b = URL(string: base), let u = URL(string: ref, relativeTo: b) {
        return u.absoluteString
    }
    if let u = URL(string: ref) { return u.absoluteString }
    return ref
}

/// Self-contained draft 2020-12 validator with annotation tracking, used for
/// schemas routed to interpretation (unevaluatedProperties/Items,
/// $dynamicRef/$dynamicAnchor, $anchor, nested $id scopes, remote $refs).
private final class JSIValidator {
    struct Ann {
        var props: Set<String> = []
        var prefix: Int = 0
        var all: Bool = false
        var idxs: Set<Int> = []
    }
    private struct Limit: Error {}

    private static let skipKeys: Set<String> = ["const", "enum", "default", "examples"]
    private static let nameMaps: Set<String> = ["properties", "patternProperties", "$defs", "definitions", "dependentSchemas"]

    private let rootSchema: JSIVal
    private let rootBase: String
    private var resources: [String: JSIVal] = [:]
    private var anchors: [String: JSIVal] = [:]
    private var dynAnchors: [String: [String: JSIVal]] = [:]
    private var baseOf: [ObjectIdentifier: String] = [:]
    private var assertions = true
    private var depth = 0
    private var regexCache: [String: NSRegularExpression?] = [:]

    init(root: JSIVal, remotes: [(String, JSIVal)]) {
        rootSchema = root
        var rb = "urn:jsi:root"
        if case .obj(let o) = root.kind, let idv = o["$id"], case .str(let id) = idv.kind {
            rb = jsiStripFrag(jsiResolveUri("urn:jsi:root", id))
        }
        rootBase = rb
        resources[rootBase] = root
        indexSchema(root, rootBase)
        for (uri, doc) in remotes {
            let u = jsiStripFrag(uri)
            if resources[u] == nil { resources[u] = doc }
            indexSchema(doc, u)
        }
        // Honor the dialect's $vocabulary: if the validation vocabulary is
        // absent, assertion keywords (type, const, minimum, ...) do not apply.
        if case .obj(let o) = root.kind, let sv = o["$schema"], case .str(let dialect) = sv.kind,
           let meta = resources[jsiStripFrag(dialect)], case .obj(let mo) = meta.kind,
           let vv = mo["$vocabulary"], case .obj(let vocab) = vv.kind {
            assertions = vocab.contains { (k, v) in
                guard k.contains("/vocab/validation") else { return false }
                if case .bool(false) = v.kind { return false }
                return true
            }
        }
    }

    // Registry pass: walk schema + remotes indexing $id resources, $anchor,
    // $dynamicAnchor. Keyword-aware: const/enum/default/examples values are
    // data; keys of properties/patternProperties/$defs/... are names.
    private func indexSchema(_ node: JSIVal, _ base: String) {
        switch node.kind {
        case .arr(let a):
            for n in a { indexSchema(n, base) }
        case .obj(let o):
            var cur = base
            if let idv = o["$id"], case .str(let id) = idv.kind {
                cur = jsiStripFrag(jsiResolveUri(base, id))
                resources[cur] = node
            }
            baseOf[ObjectIdentifier(node)] = cur
            if let av = o["$anchor"], case .str(let a) = av.kind {
                anchors[cur + "#" + a] = node
            }
            if let dv = o["$dynamicAnchor"], case .str(let da) = dv.kind {
                dynAnchors[cur, default: [:]][da] = node
                anchors[cur + "#" + da] = node
            }
            for (k, v) in o {
                if Self.skipKeys.contains(k) { continue }
                if Self.nameMaps.contains(k) {
                    if case .obj(let m) = v.kind {
                        for (_, sub) in m { indexSchema(sub, cur) }
                    }
                    continue
                }
                indexSchema(v, cur)
            }
        default:
            break
        }
    }

    private func ptrGet(_ doc: JSIVal, _ ptr: String) -> JSIVal? {
        if ptr.isEmpty { return doc }
        var cur = doc
        let segs = ptr.split(separator: "/", omittingEmptySubsequences: false).dropFirst()
        for rawSeg in segs {
            var seg = String(rawSeg).removingPercentEncoding ?? String(rawSeg)
            seg = seg.replacingOccurrences(of: "~1", with: "/")
                .replacingOccurrences(of: "~0", with: "~")
            switch cur.kind {
            case .arr(let a):
                guard let i = Int(seg), i >= 0, i < a.count else { return nil }
                cur = a[i]
            case .obj(let o):
                guard let v = o[seg] else { return nil }
                cur = v
            default:
                return nil
            }
        }
        return cur
    }

    private struct Resolved {
        let schema: JSIVal
        let base: String
    }

    private func resolveRef(_ ref: String, _ base: String) -> Resolved? {
        let uri = jsiResolveUri(base, ref)
        let frag: String
        let docUri: String
        if let i = uri.firstIndex(of: "#") {
            frag = String(uri[uri.index(after: i)...])
            docUri = String(uri[..<i])
        } else {
            frag = ""
            docUri = uri
        }
        if !frag.isEmpty && !frag.hasPrefix("/") {
            guard let target = anchors[docUri + "#" + frag] else { return nil }
            return Resolved(schema: target, base: baseOf[ObjectIdentifier(target)] ?? docUri)
        }
        guard let doc = resources[docUri] else { return nil }
        guard let target = ptrGet(doc, frag) else { return nil }
        return Resolved(schema: target, base: baseOf[ObjectIdentifier(target)] ?? docUri)
    }

    private func regex(_ pat: String) -> NSRegularExpression? {
        if let cached = regexCache[pat] { return cached }
        let r = try? NSRegularExpression(pattern: pat)
        regexCache[pat] = r
        return r
    }

    private func test(_ re: NSRegularExpression, _ s: String) -> Bool {
        // Patterns are UNANCHORED per JSON Schema — search, not whole-match.
        re.firstMatch(in: s, options: [], range: NSRange(s.startIndex..<s.endIndex, in: s)) != nil
    }

    func validate(_ instance: JSIVal) -> Bool {
        var ann = Ann()
        do {
            return try vs(rootSchema, instance, rootBase, [], &ann)
        } catch {
            return false
        }
    }

    private func vs(_ schema: JSIVal, _ inst: JSIVal, _ base: String, _ dscope: [String], _ ann: inout Ann) throws -> Bool {
        guard case .obj(let s) = schema.kind else {
            if case .bool(let b) = schema.kind { return b }
            return true
        }
        depth += 1
        defer { depth -= 1 }
        if depth > 512 { throw Limit() }

        let myBase = baseOf[ObjectIdentifier(schema)] ?? base
        var scope = dscope
        if scope.last != myBase { scope.append(myBase) }

        var local = Ann()
        func merge(_ child: Ann) {
            local.props.formUnion(child.props)
            local.idxs.formUnion(child.idxs)
            local.prefix = max(local.prefix, child.prefix)
            local.all = local.all || child.all
        }
        func sub(_ sch: JSIVal, _ i: JSIVal) throws -> (ok: Bool, ann: Ann) {
            var a = Ann()
            let ok = try vs(sch, i, myBase, scope, &a)
            return (ok, a)
        }

        // --- $ref / $dynamicRef (in-place applicators) ---
        if let rv = s["$ref"], case .str(let ref) = rv.kind {
            guard let r = resolveRef(ref, myBase) else { return false }
            var a = Ann()
            guard try vs(r.schema, inst, r.base, scope, &a) else { return false }
            merge(a)
        }
        if let dv = s["$dynamicRef"], case .str(let dref) = dv.kind {
            guard let r = resolveRef(dref, myBase) else { return false }
            var target = r.schema
            var tBase = r.base
            let frag = dref.firstIndex(of: "#").map { String(dref[dref.index(after: $0)...]) } ?? ""
            let isPlain = !frag.isEmpty && !frag.hasPrefix("/")
            if isPlain, case .obj(let tObj) = target.kind,
               let dav = tObj["$dynamicAnchor"], case .str(let da) = dav.kind, da == frag {
                // Re-resolve to the OUTERMOST dynamic-scope resource that has a
                // matching $dynamicAnchor.
                for res in scope {
                    if let m = dynAnchors[res], let t = m[frag] {
                        target = t
                        tBase = baseOf[ObjectIdentifier(t)] ?? res
                        break
                    }
                }
            }
            var a = Ann()
            guard try vs(target, inst, tBase, scope, &a) else { return false }
            merge(a)
        }

        // --- type ---
        if assertions, let tv = s["type"] {
            var types: [String] = []
            if case .str(let t) = tv.kind { types = [t] }
            if case .arr(let arr) = tv.kind {
                types = arr.compactMap { if case .str(let t) = $0.kind { return t } else { return nil } }
            }
            let ok = types.contains { t -> Bool in
                switch t {
                case "null": if case .null = inst.kind { return true }; return false
                case "boolean": if case .bool = inst.kind { return true }; return false
                case "string": if case .str = inst.kind { return true }; return false
                case "number": if case .num = inst.kind { return true }; return false
                case "integer":
                    if case .num(let d) = inst.kind { return d.isFinite && d.rounded() == d }
                    return false
                case "array": if case .arr = inst.kind { return true }; return false
                case "object": if case .obj = inst.kind { return true }; return false
                default: return false
                }
            }
            if !ok { return false }
        }

        // --- const / enum ---
        if assertions, let cv = s["const"], !jsiEqual(inst, cv) { return false }
        if assertions, let ev = s["enum"], case .arr(let cases) = ev.kind,
           !cases.contains(where: { jsiEqual(inst, $0) }) { return false }

        // --- numeric ---
        if assertions, case .num(let n) = inst.kind {
            if let v = s["multipleOf"], case .num(let m) = v.kind {
                let q = n / m
                if !q.isFinite || q != q.rounded() { return false }
            }
            if let v = s["minimum"], case .num(let b) = v.kind, n < b { return false }
            if let v = s["maximum"], case .num(let b) = v.kind, n > b { return false }
            if let v = s["exclusiveMinimum"], case .num(let b) = v.kind, n <= b { return false }
            if let v = s["exclusiveMaximum"], case .num(let b) = v.kind, n >= b { return false }
        }

        // --- string ---
        if assertions, case .str(let str) = inst.kind {
            // Lengths count Unicode code points (scalars), not graphemes.
            let cp = Double(str.unicodeScalars.count)
            if let v = s["minLength"], case .num(let b) = v.kind, cp < b { return false }
            if let v = s["maxLength"], case .num(let b) = v.kind, cp > b { return false }
            if let pv = s["pattern"], case .str(let pat) = pv.kind,
               let re = regex(pat), !test(re, str) { return false }
        }

        // --- array ---
        if case .arr(let items) = inst.kind {
            if assertions {
                if let v = s["minItems"], case .num(let b) = v.kind, Double(items.count) < b { return false }
                if let v = s["maxItems"], case .num(let b) = v.kind, Double(items.count) > b { return false }
                if let uv = s["uniqueItems"], case .bool(true) = uv.kind {
                    for i in 0..<items.count {
                        for j in (i + 1)..<items.count where jsiEqual(items[i], items[j]) {
                            return false
                        }
                    }
                }
            }
            var prefix: [JSIVal] = []
            if let pv = s["prefixItems"], case .arr(let p) = pv.kind { prefix = p }
            var i = 0
            while i < min(prefix.count, items.count) {
                if try !sub(prefix[i], items[i]).ok { return false }
                i += 1
            }
            if prefix.count > 0 {
                local.prefix = max(local.prefix, min(prefix.count, items.count))
            }
            if let iv = s["items"] {
                var j = prefix.count
                while j < items.count {
                    if try !sub(iv, items[j]).ok { return false }
                    j += 1
                }
                local.all = true
            }
            if let cv = s["contains"] {
                var matched: [Int] = []
                for (idx, item) in items.enumerated() {
                    if try sub(cv, item).ok { matched.append(idx) }
                }
                var minC = 1.0
                var maxC = Double.infinity
                if let v = s["minContains"], case .num(let b) = v.kind { minC = b }
                if let v = s["maxContains"], case .num(let b) = v.kind { maxC = b }
                if Double(matched.count) < minC || Double(matched.count) > maxC { return false }
                for idx in matched { local.idxs.insert(idx) }
            }
        }

        // --- object ---
        if case .obj(let io) = inst.kind {
            let keys = Array(io.keys)
            if assertions {
                if let v = s["minProperties"], case .num(let b) = v.kind, Double(keys.count) < b { return false }
                if let v = s["maxProperties"], case .num(let b) = v.kind, Double(keys.count) > b { return false }
                if let rv = s["required"], case .arr(let reqs) = rv.kind {
                    for r in reqs {
                        if case .str(let k) = r.kind, io[k] == nil { return false }
                    }
                }
                if let dv = s["dependentRequired"], case .obj(let dr) = dv.kind {
                    for (k, reqsV) in dr where io[k] != nil {
                        if case .arr(let reqs) = reqsV.kind {
                            for r in reqs {
                                if case .str(let rk) = r.kind, io[rk] == nil { return false }
                            }
                        }
                    }
                }
            }
            var props: [String: JSIVal] = [:]
            if let pv = s["properties"], case .obj(let p) = pv.kind { props = p }
            var patProps: [String: JSIVal] = [:]
            if let pv = s["patternProperties"], case .obj(let p) = pv.kind { patProps = p }
            for k in keys {
                var matchedLexical = false
                if let psch = props[k] {
                    if try !sub(psch, io[k]!).ok { return false }
                    matchedLexical = true
                }
                for (pat, psch) in patProps {
                    guard let re = regex(pat) else { continue }
                    if test(re, k) {
                        if try !sub(psch, io[k]!).ok { return false }
                        matchedLexical = true
                    }
                }
                if matchedLexical { local.props.insert(k) }
                if !matchedLexical, let ap = s["additionalProperties"] {
                    if try !sub(ap, io[k]!).ok { return false }
                    local.props.insert(k)
                }
            }
            if let pn = s["propertyNames"] {
                for k in keys {
                    if try !sub(pn, JSIVal(.str(k))).ok { return false }
                }
            }
            if let dsv = s["dependentSchemas"], case .obj(let ds) = dsv.kind {
                for (k, dsch) in ds where io[k] != nil {
                    let r = try sub(dsch, inst)
                    if !r.ok { return false }
                    merge(r.ann)
                }
            }
        }

        // --- in-place applicators ---
        if let av = s["allOf"], case .arr(let branches) = av.kind {
            for branch in branches {
                let r = try sub(branch, inst)
                if !r.ok { return false }
                merge(r.ann)
            }
        }
        if let av = s["anyOf"], case .arr(let branches) = av.kind {
            var any = false
            for branch in branches {
                let r = try sub(branch, inst)
                if r.ok {
                    any = true
                    merge(r.ann)
                }
            }
            if !any { return false }
        }
        if let ov = s["oneOf"], case .arr(let branches) = ov.kind {
            var count = 0
            var winner: Ann? = nil
            for branch in branches {
                let r = try sub(branch, inst)
                if r.ok {
                    count += 1
                    winner = r.ann
                }
            }
            if count != 1 { return false }
            if let w = winner { merge(w) }
        }
        if let nv = s["not"] {
            if try sub(nv, inst).ok { return false }
        }
        if let iv = s["if"] {
            let ifR = try sub(iv, inst)
            if ifR.ok {
                merge(ifR.ann)
                if let tv = s["then"] {
                    let r = try sub(tv, inst)
                    if !r.ok { return false }
                    merge(r.ann)
                }
            } else if let ev = s["else"] {
                let r = try sub(ev, inst)
                if !r.ok { return false }
                merge(r.ann)
            }
        }

        // --- unevaluated* (run last, see everything merged above) ---
        if let up = s["unevaluatedProperties"], case .obj(let io) = inst.kind {
            for k in io.keys where !local.props.contains(k) {
                if try !sub(up, io[k]!).ok { return false }
                local.props.insert(k)
            }
        }
        if let ui = s["unevaluatedItems"], case .arr(let items) = inst.kind {
            for i in 0..<items.count {
                let covered = local.all || i < local.prefix || local.idxs.contains(i)
                if !covered {
                    if try !sub(ui, items[i]).ok { return false }
                }
            }
            local.all = true
        }

        ann.props.formUnion(local.props)
        ann.idxs.formUnion(local.idxs)
        ann.prefix = max(ann.prefix, local.prefix)
        ann.all = ann.all || local.all
        return true
    }
}
