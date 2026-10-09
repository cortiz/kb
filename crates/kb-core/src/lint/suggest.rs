//! "Did you mean" suggestions for misspelled values and keys.

/// The closest candidate to `input`: a candidate that is a prefix of the input wins, otherwise
/// the nearest one within edit distance 2.
pub fn suggest<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let input = input.to_lowercase();
    if input.is_empty() {
        return None;
    }
    if let Some(c) = candidates
        .iter()
        .filter(|c| c.len() >= 2 && input.starts_with(**c))
        .max_by_key(|c| c.len())
    {
        return Some(c);
    }
    candidates
        .iter()
        .map(|c| (edit_distance(&input, c), *c))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Edit distance counted in characters: insertions, deletions, substitutions, and swaps of two
/// adjacent characters each cost 1 (optimal string alignment), so `cdm` is 1 away from `cmd`.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j - 1] + cost)
                .min(d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: &[&str] = &["sql", "sumo", "cw", "cmd", "prompt", "link", "code"];
    const KEYS: &[&str] = &["kb_format", "type", "title", "tags", "vars"];

    #[test]
    fn prefix_wins() {
        assert_eq!(suggest("sumologic", TYPES), Some("sumo"));
    }

    #[test]
    fn close_typo() {
        assert_eq!(suggest("cdm", TYPES), Some("cmd"));
        assert_eq!(suggest("tilte", KEYS), Some("title"));
        assert_eq!(suggest("SQL", TYPES), Some("sql"));
    }

    #[test]
    fn nothing_close() {
        assert_eq!(suggest("zzzz", TYPES), None);
        assert_eq!(suggest("", TYPES), None);
    }

    #[test]
    fn distance() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("é", "e"), 1);
        assert_eq!(edit_distance("cdm", "cmd"), 1);
    }
}
