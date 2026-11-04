//! The Rust op applier of the ops of a watch call (contract §6.20, "the op applier"): Chord's seven
//! ops over plain JSON, pinned by the vectors of `delta_tests.rs`.

use std::fmt;

use serde_json::Value;

/// An op the applier refused: the message names why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaError(pub String);

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "refused op: {}", self.0)
    }
}

impl std::error::Error for DeltaError {}

/// Applies the ops in order. Pure: returns the new value, or the first refused op's error; never a
/// partial result.
pub fn apply(value: Value, ops: &[Value]) -> Result<Value, DeltaError> {
    let mut value = value;
    for op in ops {
        apply_one(&mut value, op)?;
    }
    Ok(value)
}

fn refuse<T>(message: impl Into<String>) -> Result<T, DeltaError> {
    Err(DeltaError(message.into()))
}

/// A path segment: an object key or an array index.
enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

fn segments(path: &Value) -> Result<Vec<Seg<'_>>, DeltaError> {
    let Some(items) = path.as_array() else {
        return refuse("a path is not an array");
    };
    items
        .iter()
        .map(|item| match item {
            Value::String(key) => {
                if matches!(key.as_str(), "__proto__" | "constructor" | "prototype") {
                    refuse(format!("reserved segment {key:?}"))
                } else {
                    Ok(Seg::Key(key))
                }
            }
            Value::Number(n) => match n.as_u64().and_then(|n| usize::try_from(n).ok()) {
                Some(index) => Ok(Seg::Index(index)),
                None => refuse("an array index is not a non-negative integer"),
            },
            _ => refuse("a path segment is neither a string nor an integer"),
        })
        .collect()
}

fn child<'v>(value: &'v mut Value, seg: &Seg<'_>) -> Result<&'v mut Value, DeltaError> {
    match (value, seg) {
        (Value::Object(map), Seg::Key(key)) => match map.get_mut(*key) {
            Some(next) => Ok(next),
            None => refuse(format!("missing key {key:?}")),
        },
        (Value::Array(items), Seg::Index(index)) => match items.get_mut(*index) {
            Some(next) => Ok(next),
            None => refuse(format!("index {index} out of range")),
        },
        _ => refuse("a segment does not match its container"),
    }
}

fn walk<'v>(root: &'v mut Value, path: &[Seg<'_>]) -> Result<&'v mut Value, DeltaError> {
    let mut at = root;
    for seg in path {
        at = child(at, seg)?;
    }
    Ok(at)
}

fn count(value: &Value, what: &str) -> Result<usize, DeltaError> {
    match value.as_u64().and_then(|n| usize::try_from(n).ok()) {
        Some(n) => Ok(n),
        None => refuse(format!("{what} is not a non-negative integer")),
    }
}

fn apply_one(root: &mut Value, op: &Value) -> Result<(), DeltaError> {
    let Some(parts) = op.as_array() else {
        return refuse("an op is not a tuple");
    };
    let Some(tag) = parts.first().and_then(Value::as_str) else {
        return refuse("an op has no tag");
    };
    let arity = match tag {
        "r" | "d" => 2,
        "s" | "a" | "t" | "m" => 3,
        "p" => 5,
        _ => return refuse(format!("unknown op {tag:?}")),
    };
    if parts.len() != arity {
        return refuse(format!("op {tag:?} takes {arity} items"));
    }
    if tag == "r" {
        *root = parts[1].clone();
        return Ok(());
    }
    let path = segments(&parts[1])?;
    match tag {
        "s" | "d" => {
            let Some((last, parent)) = path.split_last() else {
                return refuse(format!("op {tag:?} needs a non-empty path"));
            };
            let container = walk(root, parent)?;
            match (container, last) {
                (Value::Object(map), Seg::Key(key)) => {
                    if tag == "s" {
                        map.insert((*key).to_owned(), parts[2].clone());
                    } else if map.remove(*key).is_none() {
                        return refuse(format!("missing key {key:?}"));
                    }
                }
                (Value::Array(items), Seg::Index(index)) => {
                    if *index >= items.len() {
                        return refuse(format!("index {index} out of range"));
                    }
                    if tag == "s" {
                        items[*index] = parts[2].clone();
                    } else {
                        items.remove(*index);
                    }
                }
                _ => return refuse("a segment does not match its container"),
            }
        }
        "a" | "t" => {
            if path.is_empty() {
                return refuse(format!("op {tag:?} needs a non-empty path"));
            }
            let Value::String(text) = walk(root, &path)? else {
                return refuse("the target is not a string");
            };
            if tag == "a" {
                let Some(extra) = parts[2].as_str() else {
                    return refuse("the appended value is not a string");
                };
                text.push_str(extra);
            } else {
                let n = count(&parts[2], "the trim count")?;
                let units: Vec<u16> = text.encode_utf16().collect();
                if n > units.len() {
                    return refuse("the trim count is past the end of the string");
                }
                match String::from_utf16(&units[n..]) {
                    Ok(rest) => *text = rest,
                    Err(_) => return refuse("the trim cuts a surrogate pair"),
                }
            }
        }
        "p" => {
            let Value::Array(items) = walk(root, &path)? else {
                return refuse("the target is not an array");
            };
            let start = count(&parts[2], "the start")?;
            let delete = count(&parts[3], "the delete count")?;
            let Some(new) = parts[4].as_array() else {
                return refuse("the items are not an array");
            };
            if start > items.len() || delete > items.len() - start {
                return refuse("the splice is out of range");
            }
            items.splice(start..start + delete, new.iter().cloned());
        }
        _ => {
            let Value::Array(items) = walk(root, &path)? else {
                return refuse("the target is not an array");
            };
            let Some(perm) = parts[2].as_array() else {
                return refuse("the permutation is not an array");
            };
            if perm.len() != items.len() {
                return refuse("the permutation length differs from the array");
            }
            let mut seen = vec![false; items.len()];
            let mut order = Vec::with_capacity(perm.len());
            for p in perm {
                let i = count(p, "a permutation item")?;
                if i >= seen.len() || seen[i] {
                    return refuse("not a permutation");
                }
                seen[i] = true;
                order.push(i);
            }
            let old = items.clone();
            for (slot, i) in items.iter_mut().zip(order) {
                *slot = old[i].clone();
            }
        }
    }
    Ok(())
}
