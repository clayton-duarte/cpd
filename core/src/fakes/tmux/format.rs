//! Format (`#{..}`) evaluation: `#{var}`, `#{@opt}`, `#{?c,a,b}`, `#{==:a,b}`, `#{!=:}`,
//! `#{&&:}`, `#{||:}`, `#{!:}`, `#{n:}`. `#{e|..}` and `#{P/i/:..}` are kept as written.

/// Expands `fmt`; `var` gives a variable's value, `None` when unknown (it prints empty).
pub fn expand(fmt: &str, var: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while let Some(at) = rest.find('#') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(inner_and_rest) = rest.strip_prefix("#{") {
            if let Some(end) = matching_brace(inner_and_rest) {
                let inner = &inner_and_rest[..end];
                out.push_str(&eval(inner, var).unwrap_or_else(|| format!("#{{{inner}}}")));
                rest = &inner_and_rest[end + 1..];
                continue;
            }
        } else if let Some(r) = rest.strip_prefix("##") {
            out.push('#');
            rest = r;
            continue;
        }
        out.push('#');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// The index of the `}` closing a `#{` whose body starts `s`.
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Splits on the commas outside `{..}`.
fn split_commas(s: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut from) = (Vec::new(), 0usize, 0);
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[from..]);
    parts
}

pub fn truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

fn flag(b: bool) -> String {
    if b { "1" } else { "0" }.to_string()
}

/// `None`: leave the `#{..}` as written.
fn eval(inner: &str, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(args) = inner.strip_prefix('?') {
        let parts = split_commas(args);
        let mut i = 0;
        while i + 1 < parts.len() {
            let cond = parts[i].trim();
            let value = var(cond).unwrap_or_else(|| expand(cond, var));
            if truthy(&value) {
                return Some(expand(parts[i + 1], var));
            }
            i += 2;
        }
        return Some(parts.get(i).map(|e| expand(e, var)).unwrap_or_default());
    }
    if let Some((op, args)) = inner.split_once(':') {
        if op.contains('|') && op != "||" || op.contains('/') {
            return None;
        }
        let a: Vec<String> = split_commas(args)
            .into_iter()
            .map(|p| expand(p, var))
            .collect();
        let two = |f: &dyn Fn(&str, &str) -> bool| Some(flag(f(a.first()?, a.get(1)?)));
        match op {
            "==" => return two(&|x, y| x == y),
            "!=" => return two(&|x, y| x != y),
            "&&" => return two(&|x, y| truthy(x) && truthy(y)),
            "||" => return two(&|x, y| truthy(x) || truthy(y)),
            "!" => return Some(flag(!truthy(a.first()?))),
            "n" => return Some(args_len(args, var).to_string()),
            _ => {}
        }
    }
    Some(var(inner).unwrap_or_default())
}

fn args_len(args: &str, var: &dyn Fn(&str) -> Option<String>) -> usize {
    expand(args, var).chars().count()
}
