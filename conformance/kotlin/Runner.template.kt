// AUTO-GENERATED from Runner.template.kt by conformance/kotlin/gen-runner.ts — DO NOT EDIT
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

private val strictJson = Json { ignoreUnknownKeys = true; isLenient = false }

// @jst:decoders

// @jst:excluded

private class Failure(
    val id: String,
    val keyword: String,
    val test: String,
    val expected: Boolean,
    val actual: Boolean?,
    val reason: String?,
)

fun main(args: Array<String>) {
    val manifestPath = args[0]
    val outPath = args[1]
    val t0 = System.nanoTime()

    val root = Json.parseToJsonElement(java.io.File(manifestPath).readText()).jsonObject
    val groups = root["groups"]!!.jsonArray

    var total = 0
    var pass = 0
    val failures = ArrayList<Failure>()
    val byKeyword = LinkedHashMap<String, IntArray>() // [total, pass]

    for (g in groups) {
        val go = g.jsonObject
        val id = go["id"]!!.jsonPrimitive.content
        val keyword = go["keyword"]!!.jsonPrimitive.content
        val typeName = go["type_name"]!!.jsonPrimitive.content
        val genError = (go["errors"] as? JsonObject)?.get("kotlin")?.jsonPrimitive?.content
        val decoder = decoders[typeName]
        val skipReason: String? = when {
            genError != null -> "generation error: " + genError
            decoder == null -> excluded[typeName] ?: ("no decoder for " + typeName)
            else -> null
        }
        val kw = byKeyword.getOrPut(keyword) { IntArray(2) }
        for (t in go["tests"]!!.jsonArray) {
            val to = t.jsonObject
            val desc = to["description"]!!.jsonPrimitive.content
            val expected = to["valid"]!!.jsonPrimitive.content == "true"
            total++
            kw[0]++
            if (skipReason != null || decoder == null) {
                failures.add(Failure(id, keyword, desc, expected, null, skipReason))
                continue
            }
            val dataStr = to["data"]!!.toString()
            var errMsg: String? = null
            val actual = try {
                decoder(dataStr)
                true
            } catch (e: Throwable) {
                errMsg = e.message
                false
            }
            if (actual == expected) {
                pass++
                kw[1]++
            } else {
                failures.add(Failure(id, keyword, desc, expected, actual, errMsg))
            }
        }
    }

    val elapsedMs = (System.nanoTime() - t0) / 1_000_000

    val results = buildJsonObject {
        put("language", "kotlin")
        put("total", total)
        put("pass", pass)
        put("fail", total - pass)
        put("failures", buildJsonArray {
            for (f in failures) {
                add(buildJsonObject {
                    put("id", f.id)
                    put("keyword", f.keyword)
                    put("test", f.test)
                    put("expected", f.expected)
                    if (f.actual != null) put("actual", f.actual) else put("actual", JsonNull)
                    if (f.reason != null) put("reason", f.reason)
                })
            }
        })
        put("byKeyword", buildJsonObject {
            for ((k, v) in byKeyword) {
                put(k, buildJsonObject {
                    put("total", v[0])
                    put("pass", v[1])
                })
            }
        })
    }
    java.io.File(outPath).writeText(results.toString())

    val pct = if (total > 0) "%.1f".format(100.0 * pass / total) else "0.0"
    println("kotlin conformance: $pass/$total passed ($pct%), ${total - pass} failed")
    println("run time: ${elapsedMs}ms")
    val topFailing = byKeyword.entries
        .map { Triple(it.key, it.value[0], it.value[0] - it.value[1]) }
        .filter { it.third > 0 }
        .sortedByDescending { it.third }
        .take(15)
    if (topFailing.isNotEmpty()) {
        println("top failing keywords:")
        for ((k, tot, failCount) in topFailing) {
            println("  $k: $failCount/$tot failing")
        }
    }
}
