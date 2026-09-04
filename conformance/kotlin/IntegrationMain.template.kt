import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json

private val json = Json { ignoreUnknownKeys = true; isLenient = false }

fun main() {
    val cases = listOf(
        """{"id":"a","quantity":2,"tags":["x","y"]}""" to true,
        """{"id":"a","quantity":1}""" to true,
        """{"id":"a","quantity":0}""" to false,
        """{"id":"a","quantity":1,"tags":["x","x"]}""" to false,
        """{"id":"a","quantity":1,"unknown":true}""" to false,
        """{"quantity":1}""" to false,
    )
    var ok = true
    for ((payload, expected) in cases) {
        val actual = try {
            json.decodeFromString<OrderItem>(payload)
            true
        } catch (e: Exception) {
            false
        }
        if (actual != expected) {
            ok = false
            println("MISMATCH: " + payload + " expected=" + expected + " actual=" + actual)
        }
    }
    println(if (ok) "PASS" else "FAIL")
}
