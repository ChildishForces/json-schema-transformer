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

    // Typed construction is unvalidated by design; validate() /
    // toValidatedJson() round-trip through the serializer explicitly.
    val valid = OrderItem(id = "a", quantity = 1)
    try {
        valid.validate()
    } catch (e: Exception) {
        ok = false
        println("MISMATCH: valid constructed instance failed validate(): " + e)
    }
    val validJson = try {
        valid.toValidatedJson()
    } catch (e: Exception) {
        ok = false
        println("MISMATCH: valid constructed instance failed toValidatedJson(): " + e)
        ""
    }
    if (validJson.isNotEmpty() && !validJson.contains("\"quantity\"")) {
        ok = false
        println("MISMATCH: toValidatedJson() output missing quantity: " + validJson)
    }

    val invalid = OrderItem(id = "a", quantity = 0)
    val invalidThrew = try {
        invalid.validate()
        false
    } catch (e: Exception) {
        true
    }
    if (!invalidThrew) {
        ok = false
        println("MISMATCH: invalid constructed instance passed validate()")
    }
    val invalidJsonThrew = try {
        invalid.toValidatedJson()
        false
    } catch (e: Exception) {
        true
    }
    if (!invalidJsonThrew) {
        ok = false
        println("MISMATCH: invalid constructed instance passed toValidatedJson()")
    }

    println(if (ok) "PASS" else "FAIL")
}
