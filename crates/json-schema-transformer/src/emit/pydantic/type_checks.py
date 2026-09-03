
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
