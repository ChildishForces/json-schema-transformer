type _JsiAnn = { props: Set<string>; prefix: number; all: boolean; idxs: Set<number> };

function _jsiEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    const bb = b as unknown[];
    return a.length === bb.length && a.every((v, i) => _jsiEqual(v, bb[i]));
  }
  const ao = a as Record<string, unknown>,
    bo = b as Record<string, unknown>;
  const ak = Object.keys(ao),
    bk = Object.keys(bo);
  if (ak.length !== bk.length) return false;
  return ak.every((k) => Object.prototype.hasOwnProperty.call(bo, k) && _jsiEqual(ao[k], bo[k]));
}

function _jsiValidate(
  rootSchema: unknown,
  remotes: Record<string, unknown>,
  instance: unknown
): boolean {
  const resources = new Map<string, unknown>();
  const anchors = new Map<string, unknown>();
  const dynAnchors = new Map<string, Map<string, unknown>>();
  const baseOf = new Map<object, string>();
  const SKIP = new Set(['const', 'enum', 'default', 'examples']);
  const NAME_MAPS = new Set([
    'properties',
    'patternProperties',
    '$defs',
    'definitions',
    'dependentSchemas',
  ]);

  function stripFrag(u: string): string {
    const i = u.indexOf('#');
    return i === -1 ? u : u.slice(0, i);
  }

  function resolveUri(base: string, ref: string): string {
    if (ref.startsWith('#')) return stripFrag(base) + ref;
    try {
      return new URL(ref, base || undefined).href;
    } catch {
      return ref;
    }
  }

  function indexSchema(node: unknown, base: string): void {
    if (Array.isArray(node)) {
      for (const n of node) indexSchema(n, base);
      return;
    }
    if (node === null || typeof node !== 'object') return;
    const obj = node as Record<string, unknown>;
    let cur = base;
    if (typeof obj.$id === 'string') {
      cur = stripFrag(resolveUri(base, obj.$id));
      resources.set(cur, obj);
    }
    baseOf.set(obj, cur);
    if (typeof obj.$anchor === 'string') anchors.set(cur + '#' + obj.$anchor, obj);
    if (typeof obj.$dynamicAnchor === 'string') {
      let m = dynAnchors.get(cur);
      if (!m) {
        m = new Map();
        dynAnchors.set(cur, m);
      }
      m.set(obj.$dynamicAnchor, obj);
      anchors.set(cur + '#' + obj.$dynamicAnchor, obj);
    }
    for (const [k, v] of Object.entries(obj)) {
      if (SKIP.has(k)) continue;
      if (NAME_MAPS.has(k)) {
        if (v && typeof v === 'object' && !Array.isArray(v)) {
          for (const sub of Object.values(v as Record<string, unknown>)) indexSchema(sub, cur);
        }
        continue;
      }
      indexSchema(v, cur);
    }
  }

  const ROOT_BASE = (() => {
    const id =
      rootSchema && typeof rootSchema === 'object' && !Array.isArray(rootSchema)
        ? (rootSchema as Record<string, unknown>).$id
        : undefined;
    return typeof id === 'string' ? stripFrag(resolveUri('urn:jsi:root', id)) : 'urn:jsi:root';
  })();
  resources.set(ROOT_BASE, rootSchema);
  indexSchema(rootSchema, ROOT_BASE);
  for (const [uri, doc] of Object.entries(remotes)) {
    const u = stripFrag(uri);
    if (!resources.has(u)) resources.set(u, doc);
    indexSchema(doc, u);
  }

  // Honor the dialect's $vocabulary: if the validation vocabulary is absent,
  // assertion keywords (type, const, minimum, required, ...) do not apply.
  let assertions = true;
  {
    const dialect =
      rootSchema && typeof rootSchema === 'object' && !Array.isArray(rootSchema)
        ? (rootSchema as Record<string, unknown>).$schema
        : undefined;
    if (typeof dialect === 'string') {
      const meta = resources.get(stripFrag(dialect));
      if (meta && typeof meta === 'object' && !Array.isArray(meta)) {
        const vocab = (meta as Record<string, unknown>).$vocabulary;
        if (vocab && typeof vocab === 'object' && !Array.isArray(vocab)) {
          assertions = Object.entries(vocab).some(
            ([k, v]) => k.includes('/vocab/validation') && v !== false
          );
        }
      }
    }
  }

  function ptrGet(doc: unknown, ptr: string): unknown {
    if (ptr === '') return doc;
    let cur = doc;
    for (const rawSeg of ptr.split('/').slice(1)) {
      let seg: string;
      try {
        seg = decodeURIComponent(rawSeg);
      } catch {
        seg = rawSeg;
      }
      seg = seg.replace(/~1/g, '/').replace(/~0/g, '~');
      if (Array.isArray(cur)) {
        const i = Number(seg);
        if (!Number.isInteger(i) || i < 0 || i >= cur.length) return undefined;
        cur = cur[i];
      } else if (cur !== null && typeof cur === 'object') {
        if (!Object.prototype.hasOwnProperty.call(cur, seg)) return undefined;
        cur = (cur as Record<string, unknown>)[seg];
      } else {
        return undefined;
      }
    }
    return cur;
  }

  function resolveRef(ref: string, base: string): { schema: unknown; base: string } | null {
    const uri = resolveUri(base, ref);
    const frag = uri.includes('#') ? uri.slice(uri.indexOf('#') + 1) : '';
    const docUri = stripFrag(uri);
    if (frag !== '' && !frag.startsWith('/')) {
      const target = anchors.get(docUri + '#' + frag);
      if (target === undefined) return null;
      return { schema: target, base: baseOf.get(target as object) ?? docUri };
    }
    const doc = resources.get(docUri);
    if (doc === undefined) return null;
    const target = ptrGet(doc, frag);
    if (target === undefined) return null;
    const tBase =
      target !== null && typeof target === 'object' ? baseOf.get(target as object) : undefined;
    return { schema: target, base: tBase ?? docUri };
  }

  let depth = 0;

  function vs(
    schema: unknown,
    inst: unknown,
    base: string,
    dscope: string[],
    ann: _JsiAnn
  ): boolean {
    if (schema === true) return true;
    if (schema === false) return false;
    if (schema === null || typeof schema !== 'object' || Array.isArray(schema)) return true;
    if (++depth > 512) throw new Error('schema recursion limit');
    try {
      const s = schema as Record<string, unknown>;
      let myBase = baseOf.get(schema as object) ?? base;
      let scope = dscope;
      if (scope.length === 0 || scope[scope.length - 1] !== myBase) scope = [...scope, myBase];

      const local: _JsiAnn = { props: new Set(), prefix: 0, all: false, idxs: new Set() };
      const merge = (child: _JsiAnn) => {
        for (const p of child.props) local.props.add(p);
        for (const i of child.idxs) local.idxs.add(i);
        local.prefix = Math.max(local.prefix, child.prefix);
        local.all = local.all || child.all;
      };
      const sub = (sch: unknown, i: unknown): { ok: boolean; ann: _JsiAnn } => {
        const a: _JsiAnn = { props: new Set(), prefix: 0, all: false, idxs: new Set() };
        return { ok: vs(sch, i, myBase, scope, a), ann: a };
      };

      // --- $ref / $dynamicRef (in-place applicators) ---
      if (typeof s.$ref === 'string') {
        const r = resolveRef(s.$ref, myBase);
        if (!r) return false;
        const a: _JsiAnn = { props: new Set(), prefix: 0, all: false, idxs: new Set() };
        if (!vs(r.schema, inst, r.base, scope, a)) return false;
        merge(a);
      }
      if (typeof s.$dynamicRef === 'string') {
        const r = resolveRef(s.$dynamicRef, myBase);
        if (!r) return false;
        let target = r.schema;
        let tBase = r.base;
        const frag = s.$dynamicRef.includes('#')
          ? s.$dynamicRef.slice(s.$dynamicRef.indexOf('#') + 1)
          : '';
        const isPlain = frag !== '' && !frag.startsWith('/');
        const tObj =
          target !== null && typeof target === 'object'
            ? (target as Record<string, unknown>)
            : null;
        if (isPlain && tObj && tObj.$dynamicAnchor === frag) {
          for (const res of scope) {
            const m = dynAnchors.get(res);
            if (m && m.has(frag)) {
              target = m.get(frag);
              tBase = baseOf.get(target as object) ?? res;
              break;
            }
          }
        }
        const a: _JsiAnn = { props: new Set(), prefix: 0, all: false, idxs: new Set() };
        if (!vs(target, inst, tBase, scope, a)) return false;
        merge(a);
      }

      // --- type ---
      if (assertions && s.type !== undefined) {
        const types = Array.isArray(s.type) ? (s.type as string[]) : [s.type as string];
        const ok = types.some((t) => {
          switch (t) {
            case 'null':
              return inst === null;
            case 'boolean':
              return typeof inst === 'boolean';
            case 'string':
              return typeof inst === 'string';
            case 'number':
              return typeof inst === 'number';
            case 'integer':
              return typeof inst === 'number' && Number.isInteger(inst);
            case 'array':
              return Array.isArray(inst);
            case 'object':
              return inst !== null && typeof inst === 'object' && !Array.isArray(inst);
            default:
              return false;
          }
        });
        if (!ok) return false;
      }

      // --- const / enum ---
      if (assertions && s.const !== undefined && !_jsiEqual(inst, s.const)) return false;
      if (assertions && Array.isArray(s.enum) && !s.enum.some((e) => _jsiEqual(inst, e)))
        return false;

      // --- numeric ---
      if (assertions && typeof inst === 'number') {
        if (typeof s.multipleOf === 'number') {
          const q = inst / (s.multipleOf as number);
          if (!Number.isFinite(q) || q !== Math.round(q)) return false;
        }
        if (typeof s.minimum === 'number' && inst < s.minimum) return false;
        if (typeof s.maximum === 'number' && inst > s.maximum) return false;
        if (typeof s.exclusiveMinimum === 'number' && inst <= s.exclusiveMinimum) return false;
        if (typeof s.exclusiveMaximum === 'number' && inst >= s.exclusiveMaximum) return false;
      }

      // --- string ---
      if (assertions && typeof inst === 'string') {
        const cp = () => Array.from(inst).length;
        if (typeof s.minLength === 'number' && cp() < s.minLength) return false;
        if (typeof s.maxLength === 'number' && cp() > s.maxLength) return false;
        if (typeof s.pattern === 'string') {
          let re: RegExp;
          try {
            re = new RegExp(s.pattern, 'u');
          } catch {
            try {
              re = new RegExp(s.pattern);
            } catch {
              re = /(?:)/;
            }
          }
          if (!re.test(inst)) return false;
        }
      }

      // --- array ---
      if (Array.isArray(inst)) {
        if (assertions) {
          if (typeof s.minItems === 'number' && inst.length < s.minItems) return false;
          if (typeof s.maxItems === 'number' && inst.length > s.maxItems) return false;
          if (s.uniqueItems === true) {
            for (let i = 0; i < inst.length; i++)
              for (let j = i + 1; j < inst.length; j++)
                if (_jsiEqual(inst[i], inst[j])) return false;
          }
        }
        const prefix = Array.isArray(s.prefixItems) ? (s.prefixItems as unknown[]) : [];
        for (let i = 0; i < Math.min(prefix.length, inst.length); i++) {
          if (!sub(prefix[i], inst[i]).ok) return false;
        }
        if (prefix.length > 0)
          local.prefix = Math.max(local.prefix, Math.min(prefix.length, inst.length));
        if (s.items !== undefined) {
          for (let i = prefix.length; i < inst.length; i++) {
            if (!sub(s.items, inst[i]).ok) return false;
          }
          local.all = true;
        }
        if (s.contains !== undefined) {
          const matched: number[] = [];
          for (let i = 0; i < inst.length; i++) {
            if (sub(s.contains, inst[i]).ok) matched.push(i);
          }
          const minC = typeof s.minContains === 'number' ? s.minContains : 1;
          const maxC = typeof s.maxContains === 'number' ? s.maxContains : Infinity;
          if (matched.length < minC || matched.length > maxC) return false;
          for (const i of matched) local.idxs.add(i);
        }
      }

      // --- object ---
      if (inst !== null && typeof inst === 'object' && !Array.isArray(inst)) {
        const io = inst as Record<string, unknown>;
        const keys = Object.keys(io);
        if (assertions) {
          if (typeof s.minProperties === 'number' && keys.length < s.minProperties) return false;
          if (typeof s.maxProperties === 'number' && keys.length > s.maxProperties) return false;
          if (Array.isArray(s.required)) {
            for (const k of s.required as string[]) {
              if (!Object.prototype.hasOwnProperty.call(io, k)) return false;
            }
          }
          if (s.dependentRequired && typeof s.dependentRequired === 'object') {
            for (const [k, reqs] of Object.entries(
              s.dependentRequired as Record<string, string[]>
            )) {
              if (Object.prototype.hasOwnProperty.call(io, k)) {
                for (const r of reqs)
                  if (!Object.prototype.hasOwnProperty.call(io, r)) return false;
              }
            }
          }
        }
        const props =
          s.properties && typeof s.properties === 'object'
            ? (s.properties as Record<string, unknown>)
            : {};
        const patProps =
          s.patternProperties && typeof s.patternProperties === 'object'
            ? (s.patternProperties as Record<string, unknown>)
            : {};
        for (const k of keys) {
          let matchedLexical = false;
          if (Object.prototype.hasOwnProperty.call(props, k)) {
            if (!sub(props[k], io[k]).ok) return false;
            matchedLexical = true;
          }
          for (const [pat, psch] of Object.entries(patProps)) {
            let re: RegExp;
            try {
              re = new RegExp(pat, 'u');
            } catch {
              try {
                re = new RegExp(pat);
              } catch {
                continue;
              }
            }
            if (re.test(k)) {
              if (!sub(psch, io[k]).ok) return false;
              matchedLexical = true;
            }
          }
          if (matchedLexical) local.props.add(k);
          if (!matchedLexical && s.additionalProperties !== undefined) {
            if (!sub(s.additionalProperties, io[k]).ok) return false;
            local.props.add(k);
          }
        }
        if (s.propertyNames !== undefined) {
          for (const k of keys) {
            if (!sub(s.propertyNames, k).ok) return false;
          }
        }
        if (s.dependentSchemas && typeof s.dependentSchemas === 'object') {
          for (const [k, dsch] of Object.entries(s.dependentSchemas as Record<string, unknown>)) {
            if (Object.prototype.hasOwnProperty.call(io, k)) {
              const r = sub(dsch, inst);
              if (!r.ok) return false;
              merge(r.ann);
            }
          }
        }
      }

      // --- in-place applicators ---
      if (Array.isArray(s.allOf)) {
        for (const branch of s.allOf as unknown[]) {
          const r = sub(branch, inst);
          if (!r.ok) return false;
          merge(r.ann);
        }
      }
      if (Array.isArray(s.anyOf)) {
        let any = false;
        for (const branch of s.anyOf as unknown[]) {
          const r = sub(branch, inst);
          if (r.ok) {
            any = true;
            merge(r.ann);
          }
        }
        if (!any) return false;
      }
      if (Array.isArray(s.oneOf)) {
        let count = 0;
        let winner: _JsiAnn | null = null;
        for (const branch of s.oneOf as unknown[]) {
          const r = sub(branch, inst);
          if (r.ok) {
            count++;
            winner = r.ann;
          }
        }
        if (count !== 1) return false;
        if (winner) merge(winner);
      }
      if (s.not !== undefined) {
        if (sub(s.not, inst).ok) return false;
      }
      if (s.if !== undefined) {
        const ifR = sub(s.if, inst);
        if (ifR.ok) {
          merge(ifR.ann);
          if (s.then !== undefined) {
            const r = sub(s.then, inst);
            if (!r.ok) return false;
            merge(r.ann);
          }
        } else if (s.else !== undefined) {
          const r = sub(s.else, inst);
          if (!r.ok) return false;
          merge(r.ann);
        }
      }

      // --- unevaluated* (run last, see everything merged above) ---
      if (
        s.unevaluatedProperties !== undefined &&
        inst !== null &&
        typeof inst === 'object' &&
        !Array.isArray(inst)
      ) {
        const io = inst as Record<string, unknown>;
        for (const k of Object.keys(io)) {
          if (!local.props.has(k)) {
            if (!sub(s.unevaluatedProperties, io[k]).ok) return false;
            local.props.add(k);
          }
        }
      }
      if (s.unevaluatedItems !== undefined && Array.isArray(inst)) {
        const covered = (i: number) => local.all || i < local.prefix || local.idxs.has(i);
        for (let i = 0; i < inst.length; i++) {
          if (!covered(i)) {
            if (!sub(s.unevaluatedItems, inst[i]).ok) return false;
          }
        }
        local.all = true;
      }

      for (const p of local.props) ann.props.add(p);
      for (const i of local.idxs) ann.idxs.add(i);
      ann.prefix = Math.max(ann.prefix, local.prefix);
      ann.all = ann.all || local.all;
      return true;
    } finally {
      depth--;
    }
  }

  try {
    const ann: _JsiAnn = { props: new Set(), prefix: 0, all: false, idxs: new Set() };
    return vs(rootSchema, instance, ROOT_BASE, [], ann);
  } catch {
    return false;
  }
}
