
import math as _jsi_math
import sys as _jsi_sys
from urllib.parse import unquote as _jsi_unquote, urljoin as _jsi_urljoin

if _jsi_sys.getrecursionlimit() < 10000:
    _jsi_sys.setrecursionlimit(10000)

_JSI_UNDEF = object()
_JSI_SCHEME_RE = re.compile(r"^[A-Za-z][A-Za-z0-9+.\-]*:")


class _JsiLimit(Exception):
    pass


def _jsi_num(x):
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def _jsi_equal(a, b):
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    if _jsi_num(a) and _jsi_num(b):
        return a == b
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(_jsi_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(_jsi_equal(x, y) for x, y in zip(a, b))
    return a == b


def _jsi_strip_frag(u):
    i = u.find("#")
    return u if i == -1 else u[:i]


def _jsi_resolve_uri(base, ref):
    if ref.startswith("#"):
        return _jsi_strip_frag(base) + ref
    if _JSI_SCHEME_RE.match(ref):
        return ref
    try:
        resolved = _jsi_urljoin(base, ref)
        return resolved if resolved else ref
    except Exception:
        return ref


def _jsi_compile_re(pat):
    try:
        return re.compile(pat)
    except Exception:
        return None


def _jsi_ptr_get(doc, ptr):
    if ptr == "":
        return doc
    cur = doc
    for raw in ptr.split("/")[1:]:
        try:
            seg = _jsi_unquote(raw)
        except Exception:
            seg = raw
        seg = seg.replace("~1", "/").replace("~0", "~")
        if isinstance(cur, list):
            try:
                i = int(seg)
            except (ValueError, TypeError):
                return _JSI_UNDEF
            if i < 0 or i >= len(cur):
                return _JSI_UNDEF
            cur = cur[i]
        elif isinstance(cur, dict):
            if seg not in cur:
                return _JSI_UNDEF
            cur = cur[seg]
        else:
            return _JSI_UNDEF
    return cur


def _jsi_validate(root_schema, remotes, instance):
    resources = {}
    anchors = {}
    dyn_anchors = {}
    base_of = {}
    skip_keys = {"const", "enum", "default", "examples"}
    name_maps = {"properties", "patternProperties", "$defs", "definitions", "dependentSchemas"}

    def index_schema(node, base):
        if isinstance(node, list):
            for n in node:
                index_schema(n, base)
            return
        if not isinstance(node, dict):
            return
        cur = base
        nid = node.get("$id")
        if isinstance(nid, str):
            cur = _jsi_strip_frag(_jsi_resolve_uri(base, nid))
            resources[cur] = node
        base_of[id(node)] = cur
        anc = node.get("$anchor")
        if isinstance(anc, str):
            anchors[cur + "#" + anc] = node
        dyn = node.get("$dynamicAnchor")
        if isinstance(dyn, str):
            dyn_anchors.setdefault(cur, {})[dyn] = node
            anchors[cur + "#" + dyn] = node
        for k, v in node.items():
            if k in skip_keys:
                continue
            if k in name_maps:
                if isinstance(v, dict):
                    for s in v.values():
                        index_schema(s, cur)
                continue
            index_schema(v, cur)

    root_base = "urn:jsi:root"
    if isinstance(root_schema, dict):
        rid = root_schema.get("$id")
        if isinstance(rid, str):
            root_base = _jsi_strip_frag(_jsi_resolve_uri("urn:jsi:root", rid))
    resources[root_base] = root_schema
    index_schema(root_schema, root_base)
    for uri, doc in remotes.items():
        u = _jsi_strip_frag(uri)
        if u not in resources:
            resources[u] = doc
        index_schema(doc, u)

    # Honor the dialect's $vocabulary: if the validation vocabulary is absent,
    # assertion keywords (type, const, minimum, required, ...) do not apply.
    assertions = True
    if isinstance(root_schema, dict):
        dialect = root_schema.get("$schema")
        if isinstance(dialect, str):
            meta = resources.get(_jsi_strip_frag(dialect))
            if isinstance(meta, dict):
                vocab = meta.get("$vocabulary")
                if isinstance(vocab, dict):
                    assertions = any(
                        ("/vocab/validation" in k) and v is not False
                        for k, v in vocab.items()
                    )

    def resolve_ref(ref, base):
        uri = _jsi_resolve_uri(base, ref)
        hi = uri.find("#")
        frag = uri[hi + 1:] if hi != -1 else ""
        doc_uri = _jsi_strip_frag(uri)
        if frag != "" and not frag.startswith("/"):
            target = anchors.get(doc_uri + "#" + frag)
            if target is None:
                return None
            return target, base_of.get(id(target), doc_uri)
        doc = resources.get(doc_uri, _JSI_UNDEF)
        if doc is _JSI_UNDEF:
            return None
        target = _jsi_ptr_get(doc, frag)
        if target is _JSI_UNDEF:
            return None
        tbase = base_of.get(id(target)) if isinstance(target, (dict, list)) else None
        return target, (tbase if tbase is not None else doc_uri)

    depth = [0]

    def new_ann():
        return {"props": set(), "prefix": 0, "all": False, "idxs": set()}

    def vs(schema, inst, base, dscope, ann):
        if isinstance(schema, bool):
            return schema
        if not isinstance(schema, dict):
            return True
        depth[0] += 1
        if depth[0] > 512:
            raise _JsiLimit()
        try:
            s = schema
            my_base = base_of.get(id(schema), base)
            scope = dscope
            if not scope or scope[-1] != my_base:
                scope = dscope + [my_base]

            local = new_ann()

            def merge(child):
                local["props"] |= child["props"]
                local["idxs"] |= child["idxs"]
                if child["prefix"] > local["prefix"]:
                    local["prefix"] = child["prefix"]
                local["all"] = local["all"] or child["all"]

            def sub(sch, i):
                a = new_ann()
                return vs(sch, i, my_base, scope, a), a

            # --- $ref / $dynamicRef (in-place applicators) ---
            ref = s.get("$ref")
            if isinstance(ref, str):
                r = resolve_ref(ref, my_base)
                if r is None:
                    return False
                a = new_ann()
                if not vs(r[0], inst, r[1], scope, a):
                    return False
                merge(a)

            dref = s.get("$dynamicRef")
            if isinstance(dref, str):
                r = resolve_ref(dref, my_base)
                if r is None:
                    return False
                target, tbase = r
                hi = dref.find("#")
                frag = dref[hi + 1:] if hi != -1 else ""
                is_plain = frag != "" and not frag.startswith("/")
                if is_plain and isinstance(target, dict) and target.get("$dynamicAnchor") == frag:
                    for res in scope:
                        m = dyn_anchors.get(res)
                        if m is not None and frag in m:
                            target = m[frag]
                            tbase = base_of.get(id(target), res)
                            break
                a = new_ann()
                if not vs(target, inst, tbase, scope, a):
                    return False
                merge(a)

            # --- type ---
            if assertions and "type" in s:
                tval = s["type"]
                types = tval if isinstance(tval, list) else [tval]
                ok = False
                for t in types:
                    if t == "null":
                        ok = inst is None
                    elif t == "boolean":
                        ok = isinstance(inst, bool)
                    elif t == "string":
                        ok = isinstance(inst, str)
                    elif t == "number":
                        ok = _jsi_num(inst)
                    elif t == "integer":
                        ok = _jsi_num(inst) and (isinstance(inst, int) or inst.is_integer())
                    elif t == "array":
                        ok = isinstance(inst, list)
                    elif t == "object":
                        ok = isinstance(inst, dict)
                    else:
                        ok = False
                    if ok:
                        break
                if not ok:
                    return False

            # --- const / enum ---
            if assertions and "const" in s and not _jsi_equal(inst, s["const"]):
                return False
            if assertions and isinstance(s.get("enum"), list) and not any(
                _jsi_equal(inst, e) for e in s["enum"]
            ):
                return False

            # --- numeric ---
            if assertions and _jsi_num(inst):
                mo = s.get("multipleOf")
                if _jsi_num(mo):
                    try:
                        q = inst / mo
                        if not _jsi_math.isfinite(q) or q != _jsi_math.floor(q):
                            return False
                    except (OverflowError, ZeroDivisionError):
                        return False
                mn = s.get("minimum")
                if _jsi_num(mn) and inst < mn:
                    return False
                mx = s.get("maximum")
                if _jsi_num(mx) and inst > mx:
                    return False
                emn = s.get("exclusiveMinimum")
                if _jsi_num(emn) and inst <= emn:
                    return False
                emx = s.get("exclusiveMaximum")
                if _jsi_num(emx) and inst >= emx:
                    return False

            # --- string ---
            if assertions and isinstance(inst, str):
                ml = s.get("minLength")
                if _jsi_num(ml) and len(inst) < ml:
                    return False
                mxl = s.get("maxLength")
                if _jsi_num(mxl) and len(inst) > mxl:
                    return False
                pat = s.get("pattern")
                if isinstance(pat, str):
                    rex = _jsi_compile_re(pat)
                    if rex is not None and rex.search(inst) is None:
                        return False

            # --- array ---
            if isinstance(inst, list):
                if assertions:
                    mi = s.get("minItems")
                    if _jsi_num(mi) and len(inst) < mi:
                        return False
                    mxi = s.get("maxItems")
                    if _jsi_num(mxi) and len(inst) > mxi:
                        return False
                    if s.get("uniqueItems") is True:
                        for i in range(len(inst)):
                            for j in range(i + 1, len(inst)):
                                if _jsi_equal(inst[i], inst[j]):
                                    return False
                prefix = s.get("prefixItems")
                if not isinstance(prefix, list):
                    prefix = []
                for i in range(min(len(prefix), len(inst))):
                    if not sub(prefix[i], inst[i])[0]:
                        return False
                if len(prefix) > 0:
                    local["prefix"] = max(local["prefix"], min(len(prefix), len(inst)))
                if "items" in s:
                    items = s["items"]
                    for i in range(len(prefix), len(inst)):
                        if not sub(items, inst[i])[0]:
                            return False
                    local["all"] = True
                if "contains" in s:
                    contains = s["contains"]
                    matched = [i for i in range(len(inst)) if sub(contains, inst[i])[0]]
                    minc = s.get("minContains")
                    minc = minc if _jsi_num(minc) else 1
                    maxc = s.get("maxContains")
                    if len(matched) < minc:
                        return False
                    if _jsi_num(maxc) and len(matched) > maxc:
                        return False
                    local["idxs"] |= set(matched)

            # --- object ---
            if isinstance(inst, dict):
                keys = list(inst.keys())
                if assertions:
                    mp = s.get("minProperties")
                    if _jsi_num(mp) and len(keys) < mp:
                        return False
                    mxp = s.get("maxProperties")
                    if _jsi_num(mxp) and len(keys) > mxp:
                        return False
                    req = s.get("required")
                    if isinstance(req, list):
                        for k in req:
                            if k not in inst:
                                return False
                    dr = s.get("dependentRequired")
                    if isinstance(dr, dict):
                        for k, reqs in dr.items():
                            if k in inst:
                                for rk in reqs:
                                    if rk not in inst:
                                        return False
                props = s.get("properties")
                if not isinstance(props, dict):
                    props = {}
                pat_props = s.get("patternProperties")
                if not isinstance(pat_props, dict):
                    pat_props = {}
                for k in keys:
                    matched_lex = False
                    if k in props:
                        if not sub(props[k], inst[k])[0]:
                            return False
                        matched_lex = True
                    for pat, psch in pat_props.items():
                        rex = _jsi_compile_re(pat)
                        if rex is None:
                            continue
                        if rex.search(k) is not None:
                            if not sub(psch, inst[k])[0]:
                                return False
                            matched_lex = True
                    if matched_lex:
                        local["props"].add(k)
                    elif "additionalProperties" in s:
                        if not sub(s["additionalProperties"], inst[k])[0]:
                            return False
                        local["props"].add(k)
                if "propertyNames" in s:
                    pn = s["propertyNames"]
                    for k in keys:
                        if not sub(pn, k)[0]:
                            return False
                dsv = s.get("dependentSchemas")
                if isinstance(dsv, dict):
                    for k, dsch in dsv.items():
                        if k in inst:
                            ok, a = sub(dsch, inst)
                            if not ok:
                                return False
                            merge(a)

            # --- in-place applicators ---
            all_of = s.get("allOf")
            if isinstance(all_of, list):
                for branch in all_of:
                    ok, a = sub(branch, inst)
                    if not ok:
                        return False
                    merge(a)
            any_of = s.get("anyOf")
            if isinstance(any_of, list):
                matched_any = False
                for branch in any_of:
                    ok, a = sub(branch, inst)
                    if ok:
                        matched_any = True
                        merge(a)
                if not matched_any:
                    return False
            one_of = s.get("oneOf")
            if isinstance(one_of, list):
                count = 0
                winner = None
                for branch in one_of:
                    ok, a = sub(branch, inst)
                    if ok:
                        count += 1
                        winner = a
                if count != 1:
                    return False
                if winner is not None:
                    merge(winner)
            if "not" in s:
                if sub(s["not"], inst)[0]:
                    return False
            if "if" in s:
                ok, a = sub(s["if"], inst)
                if ok:
                    merge(a)
                    if "then" in s:
                        ok2, a2 = sub(s["then"], inst)
                        if not ok2:
                            return False
                        merge(a2)
                elif "else" in s:
                    ok2, a2 = sub(s["else"], inst)
                    if not ok2:
                        return False
                    merge(a2)

            # --- unevaluated* (run last, see everything merged above) ---
            if "unevaluatedProperties" in s and isinstance(inst, dict):
                up = s["unevaluatedProperties"]
                for k in inst.keys():
                    if k not in local["props"]:
                        if not sub(up, inst[k])[0]:
                            return False
                        local["props"].add(k)
            if "unevaluatedItems" in s and isinstance(inst, list):
                ui = s["unevaluatedItems"]
                for i in range(len(inst)):
                    if not (local["all"] or i < local["prefix"] or i in local["idxs"]):
                        if not sub(ui, inst[i])[0]:
                            return False
                local["all"] = True

            ann["props"] |= local["props"]
            ann["idxs"] |= local["idxs"]
            if local["prefix"] > ann["prefix"]:
                ann["prefix"] = local["prefix"]
            ann["all"] = ann["all"] or local["all"]
            return True
        finally:
            depth[0] -= 1

    try:
        return vs(root_schema, instance, root_base, [], new_ann())
    except Exception:
        return False
