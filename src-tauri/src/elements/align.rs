//! Aligns a file's lists with another layout's lists by record size.
//!
//! Versions mostly append lists and grow structs, but sometimes insert lists
//! mid-way, so borrowing definitions by position goes wrong after the first
//! insertion. Instead, within each group of lists delimited by marker slots,
//! the longest common subsequence of record sizes anchors lists that match
//! exactly. Between two anchors, if both sides have the same number of lists,
//! they are paired by position as grown structs (the donor definition is then
//! a prefix of the record).

use super::format::{Layout, Marker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// Same record size.
    Exact(usize),
    /// Paired by position between anchors; the donor struct is smaller.
    Grown(usize),
}

impl Match {
    pub fn donor(self) -> usize {
        match self {
            Match::Exact(i) | Match::Grown(i) => i,
        }
    }
}

/// `[start, end)` ranges of list slots between marker cuts. Every marker cuts,
/// even past `count`, so a file and a shorter layout of the same family yield
/// the same number of groups.
fn groups(count: usize, markers: &[Marker]) -> Vec<(usize, usize)> {
    let mut cuts: Vec<usize> = markers.iter().map(|m| m.before).filter(|&b| b > 0).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let last = cuts.last().copied().unwrap_or(0).max(count);
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    for cut in cuts.into_iter().chain(std::iter::once(last)) {
        out.push((start.min(count), cut.min(count)));
        start = cut;
    }
    out
}

/// Pairs of equal, known (non-zero) sizes forming a longest common
/// subsequence; ties move toward the diagonal.
fn lcs(a: &[usize], b: &[usize]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[at(i, j)] = if a[i] == b[j] && a[i] > 0 {
                dp[at(i + 1, j + 1)] + 1
            } else {
                dp[at(i + 1, j)].max(dp[at(i, j + 1)])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] && a[i] > 0 && dp[at(i, j)] == dp[at(i + 1, j + 1)] + 1 {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else {
            let (down, right) = (dp[at(i + 1, j)], dp[at(i, j + 1)]);
            if down > right || (down == right && i <= j) {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    pairs
}

/// For each of the file's lists, the matching list of `donor`, if any.
pub fn align(file_sizes: &[usize], file_markers: &[Marker], donor: &Layout) -> Vec<Option<Match>> {
    let donor_sizes: Vec<usize> = donor.lists.iter().map(|l| l.as_ref().and_then(|d| d.size).unwrap_or(0)).collect();
    let fg = groups(file_sizes.len(), file_markers);
    let dg = groups(donor_sizes.len(), &donor.markers);
    let group_pairs: Vec<_> = if fg.len() == dg.len() {
        fg.into_iter().zip(dg).collect()
    } else {
        vec![((0, file_sizes.len()), (0, donor_sizes.len()))]
    };

    let mut result = vec![None; file_sizes.len()];
    for ((fs, fe), (ds, de)) in group_pairs {
        let a = &file_sizes[fs..fe];
        let b = &donor_sizes[ds..de];
        let anchors = lcs(a, b);
        // Walk the gaps before, between and after anchors.
        let mut prev: (isize, isize) = (-1, -1);
        for &(i, j) in anchors.iter().chain(std::iter::once(&(a.len(), b.len()))) {
            let (gap_a, gap_b) = (i as isize - prev.0 - 1, j as isize - prev.1 - 1);
            if gap_a > 0 && gap_a == gap_b {
                for g in 1..=gap_a {
                    let (fi, di) = ((prev.0 + g) as usize, (prev.1 + g) as usize);
                    if b[di] > 0 && b[di] < a[fi] {
                        result[fs + fi] = Some(Match::Grown(ds + di));
                    }
                }
            }
            if i < a.len() {
                result[fs + i] = Some(Match::Exact(ds + j));
            }
            prev = (i as isize, j as isize);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::format::{ListDef, MarkerKind};

    fn layout(sizes: &[usize], markers: Vec<Marker>) -> Layout {
        Layout {
            id: "t".into(),
            version: 1,
            source: String::new(),
            markers,
            lists: sizes
                .iter()
                .map(|&s| Some(ListDef { key: None, name: format!("L{s}"), struct_name: None, size: Some(s), fields: vec![] }))
                .collect(),
            enums: Default::default(),
            list_count_unverified: false,
        }
    }

    #[test]
    fn insertions_do_not_shift_later_lists() {
        let donor = layout(&[10, 20, 30, 40], vec![]);
        let got = align(&[10, 20, 99, 30, 40], &[], &donor);
        assert_eq!(got, vec![Some(Match::Exact(0)), Some(Match::Exact(1)), None, Some(Match::Exact(2)), Some(Match::Exact(3))]);
    }

    #[test]
    fn grown_structs_pair_by_position_between_anchors() {
        let donor = layout(&[10, 20, 30], vec![]);
        let got = align(&[10, 24, 30], &[], &donor);
        assert_eq!(got, vec![Some(Match::Exact(0)), Some(Match::Grown(1)), Some(Match::Exact(2))]);
    }

    #[test]
    fn markers_keep_groups_apart() {
        let m = vec![Marker { before: 2, kind: MarkerKind::Checksum }];
        let donor = layout(&[10, 20, 20, 30], m.clone());
        // Without groups, the file's second 20 could pair with the donor's first.
        let got = align(&[10, 11, 20, 30], &m, &donor);
        assert_eq!(got, vec![Some(Match::Exact(0)), None, Some(Match::Exact(2)), Some(Match::Exact(3))]);
    }
}
