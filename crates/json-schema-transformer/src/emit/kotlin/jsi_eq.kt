private fun _jsiEq(a: JsonElement, b: JsonElement): Boolean {
    if (a is JsonNull || b is JsonNull) return a is JsonNull && b is JsonNull
    if (a is JsonObject && b is JsonObject) {
        if (a.size != b.size) return false
        for ((k, v) in a) { val bv = b[k] ?: return false; if (!_jsiEq(v, bv)) return false }
        return true
    }
    if (a is JsonArray && b is JsonArray) {
        if (a.size != b.size) return false
        for (i in a.indices) if (!_jsiEq(a[i], b[i])) return false
        return true
    }
    if (a is JsonPrimitive && b is JsonPrimitive) {
        if (a.isString || b.isString) return a.isString && b.isString && a.content == b.content
        val ab = a.booleanOrNull; val bb = b.booleanOrNull
        if (ab != null || bb != null) return ab == bb
        if (a.content == b.content) return true
        val ad = a.doubleOrNull; val bd = b.doubleOrNull
        return ad != null && bd != null && ad == bd
    }
    return false
}