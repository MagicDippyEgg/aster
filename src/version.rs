//! Version comparison helpers, behaviorally equivalent to the original Python.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Num(i64),
    Text(String),
}

/// Parses a single dotted/dashed version component, splitting runs of digits and text.
fn parse_component(parsed: &mut Vec<Part>, part: &str) {
    if part.is_empty() {
        return;
    }
    if part.chars().all(|c| c.is_ascii_digit()) {
        parsed.push(Part::Num(part.parse::<i64>().unwrap_or(0)));
        return;
    }
    let mut runs: Vec<(bool, String)> = Vec::new();
    let mut cur_digit: Option<bool> = None;
    let mut cur = String::new();
    for ch in part.chars() {
        let is_digit = ch.is_ascii_digit();
        match cur_digit {
            None => {
                cur.push(ch);
                cur_digit = Some(is_digit);
            }
            Some(d) if d == is_digit => cur.push(ch),
            Some(_) => {
                runs.push((cur_digit.unwrap(), std::mem::take(&mut cur)));
                cur.push(ch);
                cur_digit = Some(is_digit);
            }
        }
    }
    if !cur.is_empty() {
        runs.push((cur_digit.unwrap_or(false), cur));
    }
    for (is_digit, run) in runs {
        if is_digit {
            parsed.push(Part::Num(run.parse::<i64>().unwrap_or(0)));
        } else if !run.is_empty() {
            parsed.push(Part::Text(run.to_lowercase()));
        }
    }
}

/// Parses a version string into a list of integers and component strings.
///
/// E.g. `'2.30.0-rc1'` -> `[2, 30, 0, "rc1"]`.
pub fn parse_version(v_str: &str) -> Vec<Part> {
    if v_str.is_empty() {
        return vec![Part::Num(0)];
    }
    let mut s = v_str.to_string();
    // Strip a leading 'v' (e.g. v1.2.3)
    if s.len() > 1 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'v' || bytes[0] == b'V') && bytes[1].is_ascii_digit() {
            s = s[1..].to_string();
        }
    }
    let mut parsed = Vec::new();
    for part in s.split(['-', '_', '.']) {
        parse_component(&mut parsed, part);
    }
    parsed
}

/// Compares two version strings.
///
/// Returns -1 if `v1 < v2`, 0 if equal, 1 if `v1 > v2`.
pub fn compare_versions(v1: &str, v2: &str) -> i32 {
    let p1 = parse_version(v1);
    let p2 = parse_version(v2);

    let n = p1.len().min(p2.len());
    for i in 0..n {
        match (&p1[i], &p2[i]) {
            (Part::Num(a), Part::Num(b)) => match a.cmp(b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => {}
            },
            (Part::Text(a), Part::Text(b)) => match a.cmp(b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => {}
            },
            // A number is considered higher than a string qualifier, e.g. 1.0 > 1.0-beta
            (Part::Num(_), Part::Text(_)) => return 1,
            (Part::Text(_), Part::Num(_)) => return -1,
        }
    }

    if p1.len() < p2.len() {
        // e.g. [1, 0, 0] vs [1, 0, 0, 'beta'] -> 'beta' is a pre-release qualifier
        if matches!(p2[p1.len()], Part::Text(_)) {
            return 1;
        }
        return -1;
    } else if p1.len() > p2.len() {
        if matches!(p1[p2.len()], Part::Text(_)) {
            return -1;
        }
        return 1;
    }
    0
}
