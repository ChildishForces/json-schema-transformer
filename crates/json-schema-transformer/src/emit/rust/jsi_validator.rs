// Self-contained JSON Schema Draft 2020-12 instance validator with annotation
// tracking (unevaluatedProperties/unevaluatedItems), $ref/$defs, $anchor,
// $dynamicRef/$dynamicAnchor, nested $id scopes and remote documents.
// Depends only on serde_json, regex and `_jst_eq` (emitted alongside).

#[derive(Default)]
struct _JstJsiAnn {
    props: std::collections::HashSet<String>,
    prefix: usize,
    all: bool,
    idxs: std::collections::HashSet<usize>,
}

impl _JstJsiAnn {
    fn merge(&mut self, c: &_JstJsiAnn) {
        for p in &c.props {
            self.props.insert(p.clone());
        }
        for i in &c.idxs {
            self.idxs.insert(*i);
        }
        if c.prefix > self.prefix {
            self.prefix = c.prefix;
        }
        self.all = self.all || c.all;
    }
}

fn _jst_jsi_strip_frag(u: &str) -> &str {
    match u.find('#') {
        Some(i) => &u[..i],
        None => u,
    }
}

/// Byte offset of the ':' terminating a URI scheme, if `s` starts with one.
fn _jst_jsi_scheme_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        match b {
            b':' => return Some(i),
            b if b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.' => {}
            _ => return None,
        }
    }
    None
}

/// RFC 3986 remove_dot_segments.
fn _jst_jsi_remove_dots(path: &str) -> String {
    let mut inp = path;
    let mut out = String::new();
    while !inp.is_empty() {
        if let Some(rest) = inp.strip_prefix("../") {
            inp = rest;
        } else if let Some(rest) = inp.strip_prefix("./") {
            inp = rest;
        } else if inp.starts_with("/./") {
            inp = &inp[2..];
        } else if inp == "/." {
            inp = "/";
        } else if inp.starts_with("/../") {
            inp = &inp[3..];
            match out.rfind('/') {
                Some(i) => out.truncate(i),
                None => out.clear(),
            }
        } else if inp == "/.." {
            inp = "/";
            match out.rfind('/') {
                Some(i) => out.truncate(i),
                None => out.clear(),
            }
        } else if inp == "." || inp == ".." {
            inp = "";
        } else {
            let start = usize::from(inp.starts_with('/'));
            let end = inp[start..].find('/').map(|i| i + start).unwrap_or(inp.len());
            out.push_str(&inp[..end]);
            inp = &inp[end..];
        }
    }
    out
}

/// Resolve `r` against `base` (RFC 3986 reference resolution; opaque bases like
/// `urn:...` return the reference unchanged, matching java.net.URI).
fn _jst_jsi_resolve(base: &str, r: &str) -> String {
    if r.is_empty() {
        return _jst_jsi_strip_frag(base).to_string();
    }
    if r.starts_with('#') {
        return format!("{}{}", _jst_jsi_strip_frag(base), r);
    }
    if _jst_jsi_scheme_end(r).is_some() {
        return r.to_string();
    }
    let bnf = _jst_jsi_strip_frag(base);
    let (b_scheme, after_scheme) = match _jst_jsi_scheme_end(bnf) {
        Some(i) => (&bnf[..i], &bnf[i + 1..]),
        None => ("", bnf),
    };
    // Opaque base (scheme + non-slash-rooted body, e.g. urn:...): the relative
    // reference is returned as-is.
    if !b_scheme.is_empty() && !after_scheme.starts_with('/') && !after_scheme.starts_with("//") {
        return r.to_string();
    }
    let (authority, b_path_q) = match after_scheme.strip_prefix("//") {
        Some(rest) => {
            let end = rest.find(['/', '?']).unwrap_or(rest.len());
            (Some(&rest[..end]), &rest[end..])
        }
        None => (None, after_scheme),
    };
    let b_path = match b_path_q.find('?') {
        Some(i) => &b_path_q[..i],
        None => b_path_q,
    };
    let scheme_prefix = if b_scheme.is_empty() {
        String::new()
    } else {
        format!("{}:", b_scheme)
    };
    let (r_main, r_frag) = match r.find('#') {
        Some(i) => (&r[..i], &r[i..]),
        None => (r, ""),
    };
    if r_main.starts_with("//") {
        return format!("{}{}", scheme_prefix, r);
    }
    let (r_path, r_query) = match r_main.find('?') {
        Some(i) => (&r_main[..i], &r_main[i..]),
        None => (r_main, ""),
    };
    let new_path = if r_path.starts_with('/') {
        _jst_jsi_remove_dots(r_path)
    } else if r_path.is_empty() {
        b_path.to_string()
    } else {
        let merged = if authority.is_some() && b_path.is_empty() {
            format!("/{}", r_path)
        } else {
            match b_path.rfind('/') {
                Some(i) => format!("{}{}", &b_path[..=i], r_path),
                None => r_path.to_string(),
            }
        };
        _jst_jsi_remove_dots(&merged)
    };
    let auth_part = match authority {
        Some(a) => format!("//{}", a),
        None => String::new(),
    };
    format!("{}{}{}{}{}", scheme_prefix, auth_part, new_path, r_query, r_frag)
}

/// Percent-decode; on malformed input the original string is returned.
fn _jst_jsi_pct_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = match s.get(i + 1..i + 3) {
                Some(h) => h,
                None => return s.to_string(),
            };
            match u8::from_str_radix(hex, 16) {
                Ok(b) => out.push(b),
                Err(_) => return s.to_string(),
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(d) => d,
        Err(_) => s.to_string(),
    }
}

fn _jst_jsi_regex(p: &str) -> Option<regex::Regex> {
    regex::Regex::new(p).ok()
}

struct _JstJsiV<'a> {
    resources: std::collections::HashMap<String, &'a serde_json::Value>,
    anchors: std::collections::HashMap<String, &'a serde_json::Value>,
    dyn_anchors:
        std::collections::HashMap<String, std::collections::HashMap<String, &'a serde_json::Value>>,
    base_of: std::collections::HashMap<usize, String>,
    depth: u32,
    assertions: bool,
    root_base: String,
}

impl<'a> _JstJsiV<'a> {
    fn new(root: &'a serde_json::Value, remotes: &'a [(String, serde_json::Value)]) -> Self {
        let root_base = match root.get("$id") {
            Some(serde_json::Value::String(id)) => {
                _jst_jsi_strip_frag(&_jst_jsi_resolve("urn:jsi:root", id)).to_string()
            }
            _ => "urn:jsi:root".to_string(),
        };
        let mut v = _JstJsiV {
            resources: std::collections::HashMap::new(),
            anchors: std::collections::HashMap::new(),
            dyn_anchors: std::collections::HashMap::new(),
            base_of: std::collections::HashMap::new(),
            depth: 0,
            assertions: true,
            root_base: root_base.clone(),
        };
        v.resources.insert(root_base.clone(), root);
        v.index_schema(root, &root_base);
        for (uri, doc) in remotes {
            let u = _jst_jsi_strip_frag(uri).to_string();
            v.resources.entry(u.clone()).or_insert(doc);
            v.index_schema(doc, &u);
        }
        // Honor the dialect's $vocabulary: if the validation vocabulary is
        // absent, assertion keywords (type, const, minimum, ...) do not apply.
        if let Some(serde_json::Value::String(dialect)) = root.get("$schema") {
            if let Some(meta) = v.resources.get(_jst_jsi_strip_frag(dialect)).copied() {
                if let Some(serde_json::Value::Object(vocab)) = meta.get("$vocabulary") {
                    v.assertions = vocab.iter().any(|(k, val)| {
                        k.contains("/vocab/validation")
                            && !matches!(val, serde_json::Value::Bool(false))
                    });
                }
            }
        }
        v
    }

    fn index_schema(&mut self, node: &'a serde_json::Value, base: &str) {
        match node {
            serde_json::Value::Array(a) => {
                for n in a {
                    self.index_schema(n, base);
                }
            }
            serde_json::Value::Object(o) => {
                let mut cur = base.to_string();
                if let Some(serde_json::Value::String(id)) = o.get("$id") {
                    cur = _jst_jsi_strip_frag(&_jst_jsi_resolve(base, id)).to_string();
                    self.resources.insert(cur.clone(), node);
                }
                self.base_of
                    .insert(node as *const serde_json::Value as usize, cur.clone());
                if let Some(serde_json::Value::String(anc)) = o.get("$anchor") {
                    self.anchors.insert(format!("{}#{}", cur, anc), node);
                }
                if let Some(serde_json::Value::String(dyn_name)) = o.get("$dynamicAnchor") {
                    self.dyn_anchors
                        .entry(cur.clone())
                        .or_default()
                        .insert(dyn_name.clone(), node);
                    self.anchors.insert(format!("{}#{}", cur, dyn_name), node);
                }
                for (k, val) in o {
                    if matches!(k.as_str(), "const" | "enum" | "default" | "examples") {
                        continue;
                    }
                    if matches!(
                        k.as_str(),
                        "properties" | "patternProperties" | "$defs" | "definitions"
                            | "dependentSchemas"
                    ) {
                        if let serde_json::Value::Object(m) = val {
                            for sub in m.values() {
                                self.index_schema(sub, &cur);
                            }
                        }
                        continue;
                    }
                    self.index_schema(val, &cur);
                }
            }
            _ => {}
        }
    }

    fn base_for(&self, node: &serde_json::Value) -> Option<String> {
        self.base_of
            .get(&(node as *const serde_json::Value as usize))
            .cloned()
    }

    fn ptr_get(doc: &'a serde_json::Value, ptr: &str) -> Option<&'a serde_json::Value> {
        if ptr.is_empty() {
            return Some(doc);
        }
        let mut cur = doc;
        for raw in ptr.split('/').skip(1) {
            let seg = _jst_jsi_pct_decode(raw).replace("~1", "/").replace("~0", "~");
            cur = match cur {
                serde_json::Value::Array(a) => {
                    let i: usize = seg.parse().ok()?;
                    a.get(i)?
                }
                serde_json::Value::Object(o) => o.get(&seg)?,
                _ => return None,
            };
        }
        Some(cur)
    }

    fn resolve_ref(&self, r: &str, base: &str) -> Option<(&'a serde_json::Value, String)> {
        let uri = _jst_jsi_resolve(base, r);
        let (doc_uri, frag) = match uri.find('#') {
            Some(i) => (&uri[..i], &uri[i + 1..]),
            None => (uri.as_str(), ""),
        };
        if !frag.is_empty() && !frag.starts_with('/') {
            let target = *self.anchors.get(&format!("{}#{}", doc_uri, frag))?;
            let b = self.base_for(target).unwrap_or_else(|| doc_uri.to_string());
            return Some((target, b));
        }
        let doc = *self.resources.get(doc_uri)?;
        let target = Self::ptr_get(doc, frag)?;
        let b = self.base_for(target).unwrap_or_else(|| doc_uri.to_string());
        Some((target, b))
    }

    /// Validate `inst` against `sch` in a fresh annotation scope.
    fn sub(
        &mut self,
        sch: &'a serde_json::Value,
        inst: &serde_json::Value,
        base: &str,
        scope: &[String],
    ) -> Result<(bool, _JstJsiAnn), ()> {
        let mut a = _JstJsiAnn::default();
        let ok = self.vs(sch, inst, base, scope, &mut a)?;
        Ok((ok, a))
    }

    fn vs(
        &mut self,
        schema: &'a serde_json::Value,
        inst: &serde_json::Value,
        base: &str,
        dscope: &[String],
        ann: &mut _JstJsiAnn,
    ) -> Result<bool, ()> {
        match schema {
            serde_json::Value::Bool(b) => return Ok(*b),
            serde_json::Value::Object(_) => {}
            _ => return Ok(true),
        }
        self.depth += 1;
        if self.depth > 512 {
            self.depth -= 1;
            return Err(());
        }
        let r = self.vs_obj(schema, inst, base, dscope);
        self.depth -= 1;
        match r? {
            Some(local) => {
                ann.merge(&local);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Core keyword evaluation; returns Ok(Some(annotations)) on success,
    /// Ok(None) on validation failure, Err on internal abort (recursion limit).
    fn vs_obj(
        &mut self,
        schema: &'a serde_json::Value,
        inst: &serde_json::Value,
        base: &str,
        dscope: &[String],
    ) -> Result<Option<_JstJsiAnn>, ()> {
        let s = match schema {
            serde_json::Value::Object(o) => o,
            _ => return Ok(Some(_JstJsiAnn::default())),
        };
        let my_base = self.base_for(schema).unwrap_or_else(|| base.to_string());
        let owned_scope: Vec<String>;
        let scope: &[String] = if dscope.last().map(String::as_str) == Some(my_base.as_str()) {
            dscope
        } else {
            let mut v = dscope.to_vec();
            v.push(my_base.clone());
            owned_scope = v;
            &owned_scope
        };
        let mut local = _JstJsiAnn::default();

        // --- $ref / $dynamicRef (in-place applicators) ---
        if let Some(serde_json::Value::String(ref_str)) = s.get("$ref") {
            let (target, t_base) = match self.resolve_ref(ref_str, &my_base) {
                Some(r) => r,
                None => return Ok(None),
            };
            let (ok, a) = self.sub(target, inst, &t_base, scope)?;
            if !ok {
                return Ok(None);
            }
            local.merge(&a);
        }
        if let Some(serde_json::Value::String(dyn_str)) = s.get("$dynamicRef") {
            let (mut target, mut t_base) = match self.resolve_ref(dyn_str, &my_base) {
                Some(r) => r,
                None => return Ok(None),
            };
            let frag = match dyn_str.find('#') {
                Some(i) => &dyn_str[i + 1..],
                None => "",
            };
            let is_plain = !frag.is_empty() && !frag.starts_with('/');
            let tda = match target.get("$dynamicAnchor") {
                Some(serde_json::Value::String(n)) => Some(n.as_str()),
                _ => None,
            };
            if is_plain && tda == Some(frag) {
                for res in scope {
                    if let Some(t) = self.dyn_anchors.get(res).and_then(|m| m.get(frag)).copied() {
                        target = t;
                        t_base = self.base_for(t).unwrap_or_else(|| res.clone());
                        break;
                    }
                }
            }
            let (ok, a) = self.sub(target, inst, &t_base, scope)?;
            if !ok {
                return Ok(None);
            }
            local.merge(&a);
        }

        // --- type ---
        if self.assertions {
            if let Some(type_el) = s.get("type") {
                let matches_type = |t: &str| match t {
                    "null" => inst.is_null(),
                    "boolean" => inst.is_boolean(),
                    "string" => inst.is_string(),
                    "number" => inst.is_number(),
                    "integer" => match inst {
                        serde_json::Value::Number(n) => {
                            n.is_i64()
                                || n.is_u64()
                                || n.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false)
                        }
                        _ => false,
                    },
                    "array" => inst.is_array(),
                    "object" => inst.is_object(),
                    _ => false,
                };
                let ok = match type_el {
                    serde_json::Value::Array(ts) => ts.iter().any(|t| match t {
                        serde_json::Value::String(t) => matches_type(t),
                        _ => false,
                    }),
                    serde_json::Value::String(t) => matches_type(t),
                    _ => false,
                };
                if !ok {
                    return Ok(None);
                }
            }
        }

        // --- const / enum ---
        if self.assertions {
            if let Some(const_el) = s.get("const") {
                if !_jst_eq(inst, const_el) {
                    return Ok(None);
                }
            }
            if let Some(serde_json::Value::Array(enum_el)) = s.get("enum") {
                if !enum_el.iter().any(|e| _jst_eq(inst, e)) {
                    return Ok(None);
                }
            }
        }

        let num_of = |v: Option<&serde_json::Value>| -> Option<f64> {
            match v {
                Some(serde_json::Value::Number(n)) => n.as_f64(),
                _ => None,
            }
        };

        // --- numeric ---
        if self.assertions {
            if let serde_json::Value::Number(n) = inst {
                if let Some(v) = n.as_f64() {
                    if let Some(mo) = num_of(s.get("multipleOf")) {
                        let q = v / mo;
                        if !q.is_finite() || q != q.floor() {
                            return Ok(None);
                        }
                    }
                    if let Some(mn) = num_of(s.get("minimum")) {
                        if v < mn {
                            return Ok(None);
                        }
                    }
                    if let Some(mx) = num_of(s.get("maximum")) {
                        if v > mx {
                            return Ok(None);
                        }
                    }
                    if let Some(emn) = num_of(s.get("exclusiveMinimum")) {
                        if v <= emn {
                            return Ok(None);
                        }
                    }
                    if let Some(emx) = num_of(s.get("exclusiveMaximum")) {
                        if v >= emx {
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // --- string ---
        if self.assertions {
            if let serde_json::Value::String(sv) = inst {
                let cp = sv.chars().count() as f64;
                if let Some(mnl) = num_of(s.get("minLength")) {
                    if cp < mnl {
                        return Ok(None);
                    }
                }
                if let Some(mxl) = num_of(s.get("maxLength")) {
                    if cp > mxl {
                        return Ok(None);
                    }
                }
                if let Some(serde_json::Value::String(pat)) = s.get("pattern") {
                    if let Some(re) = _jst_jsi_regex(pat) {
                        if !re.is_match(sv) {
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // --- array ---
        if let serde_json::Value::Array(arr) = inst {
            if self.assertions {
                if let Some(mni) = num_of(s.get("minItems")) {
                    if (arr.len() as f64) < mni {
                        return Ok(None);
                    }
                }
                if let Some(mxi) = num_of(s.get("maxItems")) {
                    if (arr.len() as f64) > mxi {
                        return Ok(None);
                    }
                }
                if matches!(s.get("uniqueItems"), Some(serde_json::Value::Bool(true))) {
                    for i in 0..arr.len() {
                        for j in i + 1..arr.len() {
                            if _jst_eq(&arr[i], &arr[j]) {
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            let prefix = match s.get("prefixItems") {
                Some(serde_json::Value::Array(p)) => Some(p),
                _ => None,
            };
            let plen = prefix.map(|p| p.len()).unwrap_or(0);
            if let Some(p) = prefix {
                let n = plen.min(arr.len());
                for i in 0..n {
                    if !self.sub(&p[i], &arr[i], &my_base, scope)?.0 {
                        return Ok(None);
                    }
                }
                if plen > 0 && n > local.prefix {
                    local.prefix = n;
                }
            }
            if let Some(items_el) = s.get("items") {
                for el in arr.iter().skip(plen) {
                    if !self.sub(items_el, el, &my_base, scope)?.0 {
                        return Ok(None);
                    }
                }
                local.all = true;
            }
            if let Some(contains_el) = s.get("contains") {
                let mut matched: Vec<usize> = Vec::new();
                for (i, el) in arr.iter().enumerate() {
                    if self.sub(contains_el, el, &my_base, scope)?.0 {
                        matched.push(i);
                    }
                }
                let min_c = num_of(s.get("minContains")).map(|d| d as i64).unwrap_or(1);
                let max_c = num_of(s.get("maxContains"))
                    .map(|d| d as i64)
                    .unwrap_or(i64::MAX);
                if (matched.len() as i64) < min_c || (matched.len() as i64) > max_c {
                    return Ok(None);
                }
                local.idxs.extend(matched);
            }
        }

        // --- object ---
        if let serde_json::Value::Object(obj) = inst {
            if self.assertions {
                if let Some(mnp) = num_of(s.get("minProperties")) {
                    if (obj.len() as f64) < mnp {
                        return Ok(None);
                    }
                }
                if let Some(mxp) = num_of(s.get("maxProperties")) {
                    if (obj.len() as f64) > mxp {
                        return Ok(None);
                    }
                }
                if let Some(serde_json::Value::Array(req)) = s.get("required") {
                    for r in req {
                        if let serde_json::Value::String(rs) = r {
                            if !obj.contains_key(rs) {
                                return Ok(None);
                            }
                        }
                    }
                }
                if let Some(serde_json::Value::Object(dep_req)) = s.get("dependentRequired") {
                    for (k, reqs) in dep_req {
                        if obj.contains_key(k) {
                            if let serde_json::Value::Array(reqs) = reqs {
                                for r in reqs {
                                    if let serde_json::Value::String(rs) = r {
                                        if !obj.contains_key(rs) {
                                            return Ok(None);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let props = match s.get("properties") {
                Some(serde_json::Value::Object(p)) => Some(p),
                _ => None,
            };
            let pat_props = match s.get("patternProperties") {
                Some(serde_json::Value::Object(p)) => Some(p),
                _ => None,
            };
            let ap = s.get("additionalProperties");
            for (k, val) in obj {
                let mut matched_lexical = false;
                if let Some(pv) = props.and_then(|p| p.get(k)) {
                    if !self.sub(pv, val, &my_base, scope)?.0 {
                        return Ok(None);
                    }
                    matched_lexical = true;
                }
                if let Some(pp) = pat_props {
                    for (pat, psch) in pp {
                        let re = match _jst_jsi_regex(pat) {
                            Some(re) => re,
                            None => continue,
                        };
                        if re.is_match(k) {
                            if !self.sub(psch, val, &my_base, scope)?.0 {
                                return Ok(None);
                            }
                            matched_lexical = true;
                        }
                    }
                }
                if matched_lexical {
                    local.props.insert(k.clone());
                } else if let Some(ap) = ap {
                    if !self.sub(ap, val, &my_base, scope)?.0 {
                        return Ok(None);
                    }
                    local.props.insert(k.clone());
                }
            }
            if let Some(pn) = s.get("propertyNames") {
                for k in obj.keys() {
                    let key_val = serde_json::Value::String(k.clone());
                    if !self.sub(pn, &key_val, &my_base, scope)?.0 {
                        return Ok(None);
                    }
                }
            }
            if let Some(serde_json::Value::Object(dep_sch)) = s.get("dependentSchemas") {
                for (k, dsch) in dep_sch {
                    if obj.contains_key(k) {
                        let (ok, a) = self.sub(dsch, inst, &my_base, scope)?;
                        if !ok {
                            return Ok(None);
                        }
                        local.merge(&a);
                    }
                }
            }
        }

        // --- in-place applicators ---
        if let Some(serde_json::Value::Array(all_of)) = s.get("allOf") {
            for branch in all_of {
                let (ok, a) = self.sub(branch, inst, &my_base, scope)?;
                if !ok {
                    return Ok(None);
                }
                local.merge(&a);
            }
        }
        if let Some(serde_json::Value::Array(any_of)) = s.get("anyOf") {
            let mut any = false;
            for branch in any_of {
                let (ok, a) = self.sub(branch, inst, &my_base, scope)?;
                if ok {
                    any = true;
                    local.merge(&a);
                }
            }
            if !any {
                return Ok(None);
            }
        }
        if let Some(serde_json::Value::Array(one_of)) = s.get("oneOf") {
            let mut count = 0;
            let mut winner: Option<_JstJsiAnn> = None;
            for branch in one_of {
                let (ok, a) = self.sub(branch, inst, &my_base, scope)?;
                if ok {
                    count += 1;
                    winner = Some(a);
                }
            }
            if count != 1 {
                return Ok(None);
            }
            if let Some(w) = winner {
                local.merge(&w);
            }
        }
        if let Some(not_el) = s.get("not") {
            if self.sub(not_el, inst, &my_base, scope)?.0 {
                return Ok(None);
            }
        }
        if let Some(if_el) = s.get("if") {
            let (if_ok, if_ann) = self.sub(if_el, inst, &my_base, scope)?;
            if if_ok {
                local.merge(&if_ann);
                if let Some(then_el) = s.get("then") {
                    let (ok, a) = self.sub(then_el, inst, &my_base, scope)?;
                    if !ok {
                        return Ok(None);
                    }
                    local.merge(&a);
                }
            } else if let Some(else_el) = s.get("else") {
                let (ok, a) = self.sub(else_el, inst, &my_base, scope)?;
                if !ok {
                    return Ok(None);
                }
                local.merge(&a);
            }
        }

        // --- unevaluated* (run last, see everything merged above) ---
        if let Some(up) = s.get("unevaluatedProperties") {
            if let serde_json::Value::Object(obj) = inst {
                for (k, val) in obj {
                    if !local.props.contains(k) {
                        if !self.sub(up, val, &my_base, scope)?.0 {
                            return Ok(None);
                        }
                        local.props.insert(k.clone());
                    }
                }
            }
        }
        if let Some(ui) = s.get("unevaluatedItems") {
            if let serde_json::Value::Array(arr) = inst {
                for (i, el) in arr.iter().enumerate() {
                    let covered = local.all || i < local.prefix || local.idxs.contains(&i);
                    if !covered && !self.sub(ui, el, &my_base, scope)?.0 {
                        return Ok(None);
                    }
                }
                local.all = true;
            }
        }

        Ok(Some(local))
    }
}

/// Validate `instance` against a JSON Schema Draft 2020-12 `schema`.
/// `remotes` maps absolute URIs to additional schema documents.
pub fn _jst_jsi_validate(
    schema: &serde_json::Value,
    remotes: &[(String, serde_json::Value)],
    instance: &serde_json::Value,
) -> bool {
    let mut v = _JstJsiV::new(schema, remotes);
    let root_base = v.root_base.clone();
    let mut ann = _JstJsiAnn::default();
    v.vs(schema, instance, &root_base, &[], &mut ann)
        .unwrap_or(false)
}
