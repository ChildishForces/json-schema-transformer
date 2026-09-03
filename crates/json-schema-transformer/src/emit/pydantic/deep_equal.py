
def _deep_equal(a, b):
    if type(a) is not type(b):
        # Allow int/float comparison for numeric equality only at leaf level
        if isinstance(a, (int, float)) and isinstance(b, (int, float)) and not isinstance(a, bool) and not isinstance(b, bool):
            return a == b
        return False
    if isinstance(a, dict):
        if set(a.keys()) != set(b.keys()):
            return False
        return all(_deep_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        if len(a) != len(b):
            return False
        return all(_deep_equal(ai, bi) for ai, bi in zip(a, b))
    return a == b
