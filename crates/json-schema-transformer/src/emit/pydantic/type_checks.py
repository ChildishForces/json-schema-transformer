
def _check_str(v):
    if not isinstance(v, str):
        raise ValueError("not a string")
    return v

def _check_int(v):
    if isinstance(v, bool):
        raise ValueError("boolean is not an integer")
    if isinstance(v, int):
        return v
    if isinstance(v, float) and v == int(v) and not (v != v):
        return v
    raise ValueError("not an integer")

def _check_number(v):
    if isinstance(v, bool):
        raise ValueError("boolean is not a number")
    if not isinstance(v, (int, float)):
        raise ValueError("not a number")
    return v

def _check_bool(v):
    if not isinstance(v, bool):
        raise ValueError("not a boolean")
    return v

def _check_unique_items(v):
    # JSON-value equality: true != 1, but 1 == 1.0
    if not isinstance(v, list):
        return v
    def _key(x):
        if isinstance(x, bool):
            return ("b", x)
        if isinstance(x, (int, float)):
            return ("n", float(x))
        if isinstance(x, str):
            return ("s", x)
        if isinstance(x, list):
            return ("l", tuple(_key(i) for i in x))
        if isinstance(x, dict):
            return ("d", tuple(sorted((k, _key(val)) for k, val in x.items())))
        return ("z", repr(x))
    seen = set()
    for x in v:
        k = _key(x)
        if k in seen:
            raise ValueError("uniqueItems")
        seen.add(k)
    return v
