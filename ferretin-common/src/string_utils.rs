/// Jaro-Winkler similarity, but with a bonus for case differences
pub(crate) fn case_aware_jaro_winkler(a: &str, b: &str) -> f64 {
    let base = strsim::jaro_winkler(&a.to_lowercase(), &b.to_lowercase());
    let case_penalty = a
        .chars()
        .zip(b.chars())
        .filter(|(ac, bc)| ac != bc && ac.eq_ignore_ascii_case(bc))
        .count() as f64
        * 0.02;
    base - case_penalty
}
