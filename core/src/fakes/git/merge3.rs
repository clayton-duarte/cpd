//! The text three-way merge under `merge-tree` and `rebase`: a line diff (LCS) of the base against each side;
//! hunks that neither overlap nor touch merge, the others conflict. No rename detection, no whitespace
//! options, no trimming of the lines a conflict's two sides share.

type Lines<'a> = Vec<&'a [u8]>;

fn split(text: &[u8]) -> Lines<'_> {
    text.split_inclusive(|b| *b == b'\n').collect()
}

/// `base[start..end]` became `side[with_start..with_end]`.
#[derive(Clone, Copy, Debug)]
struct Hunk {
    start: usize,
    end: usize,
    with_start: usize,
    with_end: usize,
}

/// The hunks that turn `base` into `side`, in order. Two hunks never touch: a line they share lies between.
fn diff(base: &[&[u8]], side: &[&[u8]]) -> Vec<Hunk> {
    let prefix = base.iter().zip(side).take_while(|(a, b)| a == b).count();
    let suffix = base[prefix..]
        .iter()
        .rev()
        .zip(side[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (b, s) = (
        &base[prefix..base.len() - suffix],
        &side[prefix..side.len() - suffix],
    );
    // `lcs[i][j]`: the longest common subsequence of `b[i..]` and `s[j..]`.
    let mut lcs = vec![vec![0u32; s.len() + 1]; b.len() + 1];
    for i in (0..b.len()).rev() {
        for j in (0..s.len()).rev() {
            lcs[i][j] = if b[i] == s[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut open: Option<(usize, usize)> = None;
    let mut hunks = Vec::new();
    let mut close = |open: &mut Option<(usize, usize)>, i: usize, j: usize| {
        if let Some((bi, sj)) = open.take() {
            hunks.push(Hunk {
                start: prefix + bi,
                end: prefix + i,
                with_start: prefix + sj,
                with_end: prefix + j,
            });
        }
    };
    while i < b.len() || j < s.len() {
        if i < b.len() && j < s.len() && b[i] == s[j] {
            close(&mut open, i, j);
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j == s.len() || (i < b.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
            i += 1;
        } else {
            j += 1;
        }
    }
    close(&mut open, i, j);
    hunks
}

/// `(inserted, deleted)` lines of the change from `old` to `new`.
pub(super) fn line_changes(old: &[u8], new: &[u8]) -> (usize, usize) {
    diff(&split(old), &split(new))
        .iter()
        .fold((0, 0), |(ins, del), h| {
            (ins + h.with_end - h.with_start, del + h.end - h.start)
        })
}

/// The merge of `ours` and `theirs` over `base`: the text, and whether it holds conflict blocks, which are
/// `<<<<<<< <label_ours>`, ours, `=======`, theirs, `>>>>>>> <label_theirs>`. With `favor_ours` a conflicting
/// hunk takes our side (`-X ours`) instead of a conflict block.
pub(super) fn merge3(
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    label_ours: &str,
    label_theirs: &str,
    favor_ours: bool,
) -> (Vec<u8>, bool) {
    let (base, ours, theirs) = (split(base), split(ours), split(theirs));
    // (side, hunk): side 0 is ours.
    let mut all: Vec<(usize, Hunk)> = diff(&base, &ours).into_iter().map(|h| (0, h)).collect();
    all.extend(diff(&base, &theirs).into_iter().map(|h| (1, h)));
    all.sort_by_key(|(side, h)| (h.start, h.end, *side));
    let sides = [&ours, &theirs];

    let mut groups: Vec<Vec<(usize, Hunk)>> = Vec::new();
    let mut reach = 0;
    for (side, hunk) in all {
        match groups.last_mut() {
            Some(group) if hunk.start <= reach => {
                reach = reach.max(hunk.end);
                group.push((side, hunk));
            }
            _ => {
                reach = hunk.end;
                groups.push(vec![(side, hunk)]);
            }
        }
    }

    let mut out: Vec<u8> = Vec::new();
    let mut conflicted = false;
    let mut at = 0;
    for group in groups {
        let start = group
            .iter()
            .map(|(_, h)| h.start)
            .min()
            .expect("a group has a hunk");
        let end = group
            .iter()
            .map(|(_, h)| h.end)
            .max()
            .expect("a group has a hunk");
        out.extend(base[at..start].concat());
        at = end;
        let text = |side: usize, hunk: &Hunk| sides[side][hunk.with_start..hunk.with_end].concat();
        // The hunks of one side never touch, so a group of several holds both sides.
        match group[..] {
            [(side, hunk)] => out.extend(text(side, &hunk)),
            [(0, a), (1, b)]
                if (a.start, a.end) == (b.start, b.end) && text(0, &a) == text(1, &b) =>
            {
                out.extend(text(0, &a))
            }
            _ => {
                let side_text = |side: usize| {
                    let mut text: Vec<u8> = Vec::new();
                    let mut p = start;
                    for (_, h) in group.iter().filter(|(s, _)| *s == side) {
                        text.extend(base[p..h.start].concat());
                        text.extend(sides[side][h.with_start..h.with_end].concat());
                        p = h.end;
                    }
                    text.extend(base[p..end].concat());
                    if !text.is_empty() && text.last() != Some(&b'\n') {
                        text.push(b'\n');
                    }
                    text
                };
                if favor_ours {
                    out.extend(side_text(0));
                    continue;
                }
                conflicted = true;
                out.extend(format!("<<<<<<< {label_ours}\n").into_bytes());
                out.extend(side_text(0));
                out.extend(b"=======\n");
                out.extend(side_text(1));
                out.extend(format!(">>>>>>> {label_theirs}\n").into_bytes());
            }
        }
    }
    out.extend(base[at..].concat());
    (out, conflicted)
}
