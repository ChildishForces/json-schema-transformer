import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

private val json = Json { ignoreUnknownKeys = true; isLenient = false }

fun main() {
    var ok = true
    val item = json.decodeFromString<OrderItem>("""{"id":"a","quantity":2}""")

    // Mutate quantity → 0 (violates minimum: 1). --mutable makes `element` a
    // var; mutation never re-validates until validate() is called explicitly.
    item.element = JsonObject(item.element.jsonObject + ("quantity" to JsonPrimitive(0)))
    val threw = try {
        item.validate()
        false
    } catch (e: Exception) {
        true
    }
    if (!threw) {
        ok = false
        println("MISMATCH: mutated-invalid instance passed validate()")
    }

    // Mutate back to a valid quantity — validate() passes again.
    item.element = JsonObject(item.element.jsonObject + ("quantity" to JsonPrimitive(2)))
    try {
        item.validate()
    } catch (e: Exception) {
        ok = false
        println("MISMATCH: mutated-valid instance failed validate(): " + e)
    }

    println(if (ok) "PASS" else "FAIL")
}
