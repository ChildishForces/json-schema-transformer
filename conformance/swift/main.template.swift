// AUTO-GENERATED from main.template.swift by conformance/swift/run.ts — DO NOT EDIT
import Foundation

// @jst:registrations

// ── Runner ───────────────────────────────────────────────────────────────────

let argv = CommandLine.arguments

guard argv.count >= 3 else {
    FileHandle.standardError.write(Data("usage: runner <manifest.json> <results.json> [exclusions.json]\n".utf8))
    exit(2)
}

var exclusions: [String: String] = [:]

if argv.count >= 4,
   let d = try? Data(contentsOf: URL(fileURLWithPath: argv[3])),
   let obj = (try? JSONSerialization.jsonObject(with: d)) as? [String: String] {
    exclusions = obj
}

var decoders: [String: (Data) throws -> Void] = [:]

registerAll(&decoders)

let manifestData = try Data(contentsOf: URL(fileURLWithPath: argv[1]))
let manifest = try JSONSerialization.jsonObject(with: manifestData) as! [String: Any]
let groups = manifest["groups"] as! [[String: Any]]

var total = 0
var passCount = 0
var failures: [[String: Any]] = []
var kwTotal: [String: Int] = [:]
var kwPass: [String: Int] = [:]
let runStart = Date()

for group in groups {
    let id = group["id"] as! String
    let keyword = group["keyword"] as! String
    let typeName = group["type_name"] as? String ?? ""
    let errors = group["errors"] as? [String: Any] ?? [:]
    let files = group["files"] as? [String: Any] ?? [:]
    let swiftFile = (files["swift"] as? String).map { ($0 as NSString).lastPathComponent }

    var groupReason: String? = nil
    if let genErr = errors["swift"] as? String {
        groupReason = "generation error: \(genErr)"
    } else if let f = swiftFile, let msg = exclusions[f] {
        groupReason = "compile error: \(msg)"
    } else if decoders[typeName] == nil {
        groupReason = swiftFile == nil ? "no swift file generated" : "no decoder registered"
    }

    for test in group["tests"] as? [[String: Any]] ?? [] {
        total += 1
        kwTotal[keyword, default: 0] += 1
        let desc = test["description"] as? String ?? ""
        let expected = test["valid"] as! Bool

        var actual: Bool? = nil
        var reason: String? = groupReason
        if reason == nil, let decode = decoders[typeName] {
            let dataValue = test["data"] ?? NSNull()
            do {
                let jsonData = try JSONSerialization.data(withJSONObject: dataValue, options: [.fragmentsAllowed])
                do {
                    try decode(jsonData)
                    actual = true
                } catch {
                    actual = false
                }
            } catch {
                reason = "could not serialize test data: \(error)"
            }
        }

        if reason == nil && actual == expected {
            passCount += 1
            kwPass[keyword, default: 0] += 1
        } else {
            var f: [String: Any] = [
                "id": id,
                "keyword": keyword,
                "test": desc,
                "expected": expected,
                "actual": actual.map { $0 as Any } ?? NSNull(),
            ]
            if let r = reason { f["reason"] = r }
            failures.append(f)
        }
    }
}

let runTime = Date().timeIntervalSince(runStart)

var byKeyword: [String: Any] = [:]
for (kw, t) in kwTotal {
    byKeyword[kw] = ["total": t, "pass": kwPass[kw] ?? 0]
}

let results: [String: Any] = [
    "language": "swift",
    "total": total,
    "pass": passCount,
    "fail": total - passCount,
    "failures": failures,
    "byKeyword": byKeyword,
]

let out = try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
try out.write(to: URL(fileURLWithPath: argv[2]))

let pct = total > 0 ? Double(passCount) / Double(total) * 100 : 0

print(String(format: "swift conformance: %d/%d passed (%.1f%%), %d failed", passCount, total, pct, total - passCount))

let failing = kwTotal
    .map { (kw: $0.key, total: $0.value, fails: $0.value - (kwPass[$0.key] ?? 0)) }
    .filter { $0.fails > 0 }
    .sorted { $0.fails > $1.fails }

print("top failing keywords:")

for e in failing.prefix(15) {
    print("  " + e.kw.padding(toLength: 24, withPad: " ", startingAt: 0) + " \(e.fails)/\(e.total) failing")
}

print(String(format: "run time: %.2fs", runTime))
