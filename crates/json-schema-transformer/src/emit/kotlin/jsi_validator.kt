private class _JsiAnn {
    val props = HashSet<String>()
    var prefix = 0
    var all = false
    val idxs = HashSet<Int>()
}

private fun _jsiIsStr(e: JsonElement): Boolean = e is JsonPrimitive && e !is JsonNull && e.isString
private fun _jsiIsBool(e: JsonElement): Boolean = e is JsonPrimitive && e !is JsonNull && !e.isString && e.booleanOrNull != null
private fun _jsiIsNum(e: JsonElement): Boolean = e is JsonPrimitive && e !is JsonNull && !e.isString && e.booleanOrNull == null
private fun _jsiIsInt(e: JsonElement): Boolean {
    if (!_jsiIsNum(e)) return false
    return try { java.math.BigDecimal((e as JsonPrimitive).content).stripTrailingZeros().scale() <= 0 } catch (_: Exception) { false }
}
private fun _jsiNum(e: JsonElement?): Double? =
    if (e != null && _jsiIsNum(e)) (e as JsonPrimitive).doubleOrNull else null
private fun _jsiStr(e: JsonElement?): String? =
    if (e != null && _jsiIsStr(e)) (e as JsonPrimitive).content else null

private fun _jsiStripFrag(u: String): String {
    val i = u.indexOf('#')
    return if (i == -1) u else u.substring(0, i)
}

private fun _jsiResolve(base: String, ref: String): String {
    if (ref.isEmpty()) return _jsiStripFrag(base)
    if (ref.startsWith("#")) return _jsiStripFrag(base) + ref
    return try { java.net.URI(base).resolve(ref).toString() } catch (_: Exception) { ref }
}

private fun _jsiPctDecode(s: String): String {
    if ('%' !in s) return s
    return try { java.net.URLDecoder.decode(s.replace("+", "%2B"), "UTF-8") } catch (_: Exception) { s }
}

private val _jsiPropNames = mapOf(
    "Letter" to "L", "Lowercase_Letter" to "Ll", "Uppercase_Letter" to "Lu",
    "Titlecase_Letter" to "Lt", "Modifier_Letter" to "Lm", "Other_Letter" to "Lo",
    "Mark" to "M", "Nonspacing_Mark" to "Mn", "Spacing_Mark" to "Mc", "Enclosing_Mark" to "Me",
    "Number" to "N", "Decimal_Number" to "Nd", "digit" to "Nd", "Letter_Number" to "Nl", "Other_Number" to "No",
    "Punctuation" to "P", "Connector_Punctuation" to "Pc", "Dash_Punctuation" to "Pd",
    "Open_Punctuation" to "Ps", "Close_Punctuation" to "Pe", "Initial_Punctuation" to "Pi",
    "Final_Punctuation" to "Pf", "Other_Punctuation" to "Po",
    "Symbol" to "S", "Math_Symbol" to "Sm", "Currency_Symbol" to "Sc",
    "Modifier_Symbol" to "Sk", "Other_Symbol" to "So",
    "Separator" to "Z", "Space_Separator" to "Zs", "Line_Separator" to "Zl", "Paragraph_Separator" to "Zp",
    "Other" to "C", "Control" to "Cc", "Format" to "Cf", "Surrogate" to "Cs",
    "Private_Use" to "Co", "Unassigned" to "Cn"
)

private fun _jsiRegex(p: String): java.util.regex.Pattern? {
    try { return java.util.regex.Pattern.compile(p) } catch (_: Exception) {}
    val sb = StringBuilder()
    var i = 0
    while (i < p.length) {
        if (i + 3 < p.length && p[i] == '\\' && (p[i + 1] == 'p' || p[i + 1] == 'P') && p[i + 2] == '{') {
            val close = p.indexOf('}', i + 3)
            if (close > 0) {
                val repl = _jsiPropNames[p.substring(i + 3, close)]
                if (repl != null) {
                    sb.append(p, i, i + 2).append('{').append(repl).append('}')
                    i = close + 1
                    continue
                }
            }
        }
        sb.append(p[i]); i++
    }
    return try { java.util.regex.Pattern.compile(sb.toString()) } catch (_: Exception) { null }
}

private class _JsiV(rootSchema: JsonElement, remotes: JsonObject) {
    val resources = HashMap<String, JsonElement>()
    val anchors = HashMap<String, JsonElement>()
    val dynAnchors = HashMap<String, HashMap<String, JsonElement>>()
    val baseOf = java.util.IdentityHashMap<JsonElement, String>()
    var depth = 0
    var assertions = true
    val rootBase: String

    private val skipKeys = setOf("const", "enum", "default", "examples")
    private val nameMaps = setOf("properties", "patternProperties", "\$defs", "definitions", "dependentSchemas")

    init {
        val idEl = (rootSchema as? JsonObject)?.get("\$id")
        rootBase = if (idEl != null && _jsiIsStr(idEl)) _jsiStripFrag(_jsiResolve("urn:jsi:root", (idEl as JsonPrimitive).content)) else "urn:jsi:root"
        resources[rootBase] = rootSchema
        indexSchema(rootSchema, rootBase)
        for ((uri, doc) in remotes) {
            val u = _jsiStripFrag(uri)
            if (u !in resources) resources[u] = doc
            indexSchema(doc, u)
        }
        // Honor the dialect's $vocabulary: if the validation vocabulary is
        // absent, assertion keywords (type, const, minimum, ...) do not apply.
        val dialect = (rootSchema as? JsonObject)?.get("\$schema")
        if (dialect != null && _jsiIsStr(dialect)) {
            val meta = resources[_jsiStripFrag((dialect as JsonPrimitive).content)]
            val vocab = (meta as? JsonObject)?.get("\$vocabulary")
            if (vocab is JsonObject) {
                assertions = vocab.any { (k, v) ->
                    k.contains("/vocab/validation") && !(v is JsonPrimitive && v !is JsonNull && !v.isString && v.booleanOrNull == false)
                }
            }
        }
    }

    fun indexSchema(node: JsonElement, base: String) {
        when (node) {
            is JsonArray -> for (n in node) indexSchema(n, base)
            is JsonObject -> {
                var cur = base
                val id = node["\$id"]
                if (id != null && _jsiIsStr(id)) {
                    cur = _jsiStripFrag(_jsiResolve(base, (id as JsonPrimitive).content))
                    resources[cur] = node
                }
                baseOf[node] = cur
                val anc = node["\$anchor"]
                if (anc != null && _jsiIsStr(anc)) anchors[cur + "#" + (anc as JsonPrimitive).content] = node
                val dyn = node["\$dynamicAnchor"]
                if (dyn != null && _jsiIsStr(dyn)) {
                    val name = (dyn as JsonPrimitive).content
                    dynAnchors.getOrPut(cur) { HashMap() }[name] = node
                    anchors["$cur#$name"] = node
                }
                for ((k, v) in node) {
                    if (k in skipKeys) continue
                    if (k in nameMaps) {
                        if (v is JsonObject) for (sub in v.values) indexSchema(sub, cur)
                        continue
                    }
                    indexSchema(v, cur)
                }
            }
            else -> {}
        }
    }

    fun ptrGet(doc: JsonElement, ptr: String): JsonElement? {
        if (ptr.isEmpty()) return doc
        var cur: JsonElement = doc
        for (raw in ptr.split("/").drop(1)) {
            val seg = _jsiPctDecode(raw).replace("~1", "/").replace("~0", "~")
            cur = when (cur) {
                is JsonArray -> {
                    val i = seg.toIntOrNull() ?: return null
                    if (i < 0 || i >= cur.size) return null
                    cur[i]
                }
                is JsonObject -> cur[seg] ?: return null
                else -> return null
            }
        }
        return cur
    }

    fun resolveRef(ref: String, base: String): Pair<JsonElement, String>? {
        val uri = _jsiResolve(base, ref)
        val hi = uri.indexOf('#')
        val frag = if (hi == -1) "" else uri.substring(hi + 1)
        val docUri = _jsiStripFrag(uri)
        if (frag.isNotEmpty() && !frag.startsWith("/")) {
            val target = anchors["$docUri#$frag"] ?: return null
            return Pair(target, baseOf[target] ?: docUri)
        }
        val doc = resources[docUri] ?: return null
        val target = ptrGet(doc, frag) ?: return null
        return Pair(target, baseOf[target] ?: docUri)
    }

    fun vs(schema: JsonElement, inst: JsonElement, base: String, dscope: List<String>, ann: _JsiAnn): Boolean {
        if (schema is JsonPrimitive && schema !is JsonNull && !schema.isString) {
            val b = schema.booleanOrNull
            if (b != null) return b
        }
        if (schema !is JsonObject) return true
        if (++depth > 512) throw RuntimeException("schema recursion limit")
        try {
            val s = schema
            val myBase = baseOf[schema] ?: base
            var scope = dscope
            if (scope.isEmpty() || scope.last() != myBase) scope = scope + myBase

            val local = _JsiAnn()
            fun merge(c: _JsiAnn) {
                local.props.addAll(c.props)
                local.idxs.addAll(c.idxs)
                if (c.prefix > local.prefix) local.prefix = c.prefix
                local.all = local.all || c.all
            }
            fun sub(sch: JsonElement, i: JsonElement): Pair<Boolean, _JsiAnn> {
                val a = _JsiAnn()
                return Pair(vs(sch, i, myBase, scope, a), a)
            }

            // --- $ref / $dynamicRef (in-place applicators) ---
            val refStr = _jsiStr(s["\$ref"])
            if (refStr != null) {
                val r = resolveRef(refStr, myBase) ?: return false
                val a = _JsiAnn()
                if (!vs(r.first, inst, r.second, scope, a)) return false
                merge(a)
            }
            val dynStr = _jsiStr(s["\$dynamicRef"])
            if (dynStr != null) {
                val r = resolveRef(dynStr, myBase) ?: return false
                var target = r.first
                var tBase = r.second
                val hi = dynStr.indexOf('#')
                val frag = if (hi == -1) "" else dynStr.substring(hi + 1)
                val isPlain = frag.isNotEmpty() && !frag.startsWith("/")
                val tda = _jsiStr((target as? JsonObject)?.get("\$dynamicAnchor"))
                if (isPlain && tda == frag) {
                    for (res in scope) {
                        val t = dynAnchors[res]?.get(frag)
                        if (t != null) {
                            target = t
                            tBase = baseOf[t] ?: res
                            break
                        }
                    }
                }
                val a = _JsiAnn()
                if (!vs(target, inst, tBase, scope, a)) return false
                merge(a)
            }

            // --- type ---
            val typeEl = s["type"]
            if (assertions && typeEl != null) {
                val types: List<String> = when (typeEl) {
                    is JsonArray -> typeEl.mapNotNull { _jsiStr(it) }
                    else -> _jsiStr(typeEl)?.let { listOf(it) } ?: listOf()
                }
                val ok = types.any { t ->
                    when (t) {
                        "null" -> inst is JsonNull
                        "boolean" -> _jsiIsBool(inst)
                        "string" -> _jsiIsStr(inst)
                        "number" -> _jsiIsNum(inst)
                        "integer" -> _jsiIsInt(inst)
                        "array" -> inst is JsonArray
                        "object" -> inst is JsonObject
                        else -> false
                    }
                }
                if (!ok) return false
            }

            // --- const / enum ---
            val constEl = s["const"]
            if (assertions && constEl != null && !_jsiEq(inst, constEl)) return false
            val enumEl = s["enum"]
            if (assertions && enumEl is JsonArray && enumEl.none { _jsiEq(inst, it) }) return false

            // --- numeric ---
            if (assertions && _jsiIsNum(inst)) {
                val v = (inst as JsonPrimitive).doubleOrNull
                if (v != null) {
                    val mo = _jsiNum(s["multipleOf"])
                    if (mo != null) {
                        val q = v / mo
                        if (!q.isFinite() || q != kotlin.math.floor(q)) return false
                    }
                    val mn = _jsiNum(s["minimum"]); if (mn != null && v < mn) return false
                    val mx = _jsiNum(s["maximum"]); if (mx != null && v > mx) return false
                    val emn = _jsiNum(s["exclusiveMinimum"]); if (emn != null && v <= emn) return false
                    val emx = _jsiNum(s["exclusiveMaximum"]); if (emx != null && v >= emx) return false
                }
            }

            // --- string ---
            if (assertions && _jsiIsStr(inst)) {
                val sv = (inst as JsonPrimitive).content
                val cp = sv.codePointCount(0, sv.length)
                val mnl = _jsiNum(s["minLength"]); if (mnl != null && cp < mnl) return false
                val mxl = _jsiNum(s["maxLength"]); if (mxl != null && cp > mxl) return false
                val pat = _jsiStr(s["pattern"])
                if (pat != null) {
                    val re = _jsiRegex(pat)
                    if (re != null && !re.matcher(sv).find()) return false
                }
            }

            // --- array ---
            if (inst is JsonArray) {
                if (assertions) {
                    val mni = _jsiNum(s["minItems"]); if (mni != null && inst.size < mni) return false
                    val mxi = _jsiNum(s["maxItems"]); if (mxi != null && inst.size > mxi) return false
                    val uniq = s["uniqueItems"]
                    if (uniq is JsonPrimitive && uniq !is JsonNull && !uniq.isString && uniq.booleanOrNull == true) {
                        for (i in inst.indices)
                            for (j in i + 1 until inst.size)
                                if (_jsiEq(inst[i], inst[j])) return false
                    }
                }
                val prefix = s["prefixItems"] as? JsonArray
                val plen = prefix?.size ?: 0
                if (prefix != null) {
                    val n = minOf(plen, inst.size)
                    for (i in 0 until n) if (!sub(prefix[i], inst[i]).first) return false
                    if (plen > 0 && n > local.prefix) local.prefix = n
                }
                val itemsEl = s["items"]
                if (itemsEl != null) {
                    for (i in plen until inst.size) if (!sub(itemsEl, inst[i]).first) return false
                    local.all = true
                }
                val containsEl = s["contains"]
                if (containsEl != null) {
                    val matched = ArrayList<Int>()
                    for (i in inst.indices) if (sub(containsEl, inst[i]).first) matched.add(i)
                    val minC = _jsiNum(s["minContains"])?.toInt() ?: 1
                    val maxC = _jsiNum(s["maxContains"])?.toInt() ?: Int.MAX_VALUE
                    if (matched.size < minC || matched.size > maxC) return false
                    local.idxs.addAll(matched)
                }
            }

            // --- object ---
            if (inst is JsonObject) {
                if (assertions) {
                    val mnp = _jsiNum(s["minProperties"]); if (mnp != null && inst.size < mnp) return false
                    val mxp = _jsiNum(s["maxProperties"]); if (mxp != null && inst.size > mxp) return false
                    val req = s["required"] as? JsonArray
                    if (req != null) for (r in req) {
                        val rs = _jsiStr(r) ?: continue
                        if (rs !in inst) return false
                    }
                    val depReq = s["dependentRequired"] as? JsonObject
                    if (depReq != null) for ((k, reqs) in depReq) {
                        if (k in inst && reqs is JsonArray) {
                            for (r in reqs) {
                                val rs = _jsiStr(r) ?: continue
                                if (rs !in inst) return false
                            }
                        }
                    }
                }
                val props = s["properties"] as? JsonObject
                val patProps = s["patternProperties"] as? JsonObject
                val ap = s["additionalProperties"]
                for (k in inst.keys) {
                    var matchedLexical = false
                    val pv = props?.get(k)
                    if (pv != null) {
                        if (!sub(pv, inst[k]!!).first) return false
                        matchedLexical = true
                    }
                    if (patProps != null) for ((pat, psch) in patProps) {
                        val re = _jsiRegex(pat) ?: continue
                        if (re.matcher(k).find()) {
                            if (!sub(psch, inst[k]!!).first) return false
                            matchedLexical = true
                        }
                    }
                    if (matchedLexical) local.props.add(k)
                    if (!matchedLexical && ap != null) {
                        if (!sub(ap, inst[k]!!).first) return false
                        local.props.add(k)
                    }
                }
                val pn = s["propertyNames"]
                if (pn != null) for (k in inst.keys) {
                    if (!sub(pn, JsonPrimitive(k)).first) return false
                }
                val depSch = s["dependentSchemas"] as? JsonObject
                if (depSch != null) for ((k, dsch) in depSch) {
                    if (k in inst) {
                        val r = sub(dsch, inst)
                        if (!r.first) return false
                        merge(r.second)
                    }
                }
            }

            // --- in-place applicators ---
            val allOf = s["allOf"] as? JsonArray
            if (allOf != null) for (branch in allOf) {
                val r = sub(branch, inst)
                if (!r.first) return false
                merge(r.second)
            }
            val anyOf = s["anyOf"] as? JsonArray
            if (anyOf != null) {
                var any = false
                for (branch in anyOf) {
                    val r = sub(branch, inst)
                    if (r.first) { any = true; merge(r.second) }
                }
                if (!any) return false
            }
            val oneOf = s["oneOf"] as? JsonArray
            if (oneOf != null) {
                var count = 0
                var winner: _JsiAnn? = null
                for (branch in oneOf) {
                    val r = sub(branch, inst)
                    if (r.first) { count++; winner = r.second }
                }
                if (count != 1) return false
                if (winner != null) merge(winner)
            }
            val notEl = s["not"]
            if (notEl != null && sub(notEl, inst).first) return false
            val ifEl = s["if"]
            if (ifEl != null) {
                val ifR = sub(ifEl, inst)
                if (ifR.first) {
                    merge(ifR.second)
                    val thenEl = s["then"]
                    if (thenEl != null) {
                        val r = sub(thenEl, inst)
                        if (!r.first) return false
                        merge(r.second)
                    }
                } else {
                    val elseEl = s["else"]
                    if (elseEl != null) {
                        val r = sub(elseEl, inst)
                        if (!r.first) return false
                        merge(r.second)
                    }
                }
            }

            // --- unevaluated* (run last, see everything merged above) ---
            val up = s["unevaluatedProperties"]
            if (up != null && inst is JsonObject) {
                for (k in inst.keys) {
                    if (k !in local.props) {
                        if (!sub(up, inst[k]!!).first) return false
                        local.props.add(k)
                    }
                }
            }
            val ui = s["unevaluatedItems"]
            if (ui != null && inst is JsonArray) {
                for (i in inst.indices) {
                    val covered = local.all || i < local.prefix || i in local.idxs
                    if (!covered) {
                        if (!sub(ui, inst[i]).first) return false
                    }
                }
                local.all = true
            }

            ann.props.addAll(local.props)
            ann.idxs.addAll(local.idxs)
            if (local.prefix > ann.prefix) ann.prefix = local.prefix
            ann.all = ann.all || local.all
            return true
        } finally {
            depth--
        }
    }
}

private fun _jsiValidate(rootSchema: JsonElement, remotes: JsonObject, instance: JsonElement): Boolean {
    return try {
        val v = _JsiV(rootSchema, remotes)
        v.vs(rootSchema, instance, v.rootBase, listOf(), _JsiAnn())
    } catch (_: Throwable) {
        false
    }
}