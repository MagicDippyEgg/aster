"""
Version comparison helper module for Aster.
"""

import re
from typing import Tuple, List, Union

def parse_version(v_str: str) -> List[Union[int, str]]:
    """
    Parses a version string into a list of integers and component strings for comparison.
    E.g. '2.30.0-rc1' -> [2, 30, 0, 'rc1']
    """
    if not v_str:
        return [0]

    # Strip leading 'v' if present e.g. v1.2.3
    if v_str.lower().startswith('v') and len(v_str) > 1 and v_str[1].isdigit():
        v_str = v_str[1:]

    parts = re.split(r'[-_.]', v_str)
    parsed = []
    for part in parts:
        if part.isdigit():
            parsed.append(int(part))
        else:
            # Handle alpha/numeric mix e.g. 1b -> 1, 'b'
            subparts = re.findall(r'\d+|\D+', part)
            for sub in subparts:
                if sub.isdigit():
                    parsed.append(int(sub))
                elif sub:
                    parsed.append(sub.lower())
    return parsed

def compare_versions(v1: str, v2: str) -> int:
    """
    Compares two version strings v1 and v2.
    Returns:
       -1 if v1 < v2
        0 if v1 == v2
        1 if v1 > v2
    """
    p1 = parse_version(v1)
    p2 = parse_version(v2)

    for a, b in zip(p1, p2):
        if type(a) == type(b):
            if a < b:
                return -1
            elif a > b:
                return 1
        else:
            # Number is considered higher than string qualifier e.g. 1.0 > 1.0-beta
            if isinstance(a, int) and isinstance(b, str):
                return 1
            elif isinstance(a, str) and isinstance(b, int):
                return -1

    if len(p1) < len(p2):
        # e.g. [1, 0, 0] vs [1, 0, 0, 'beta'] -> 'beta' is pre-release qualifier
        if isinstance(p2[len(p1)], str):
            return 1
        return -1
    elif len(p1) > len(p2):
        if isinstance(p1[len(p2)], str):
            return -1
        return 1
    return 0
