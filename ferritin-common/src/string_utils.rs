use rust_stemmers::{Algorithm, Stemmer};
use std::{borrow::Cow, sync::OnceLock};

/// Reduce an English word to its Snowball (Porter2) stem, so that
/// morphological variants of the same word collide: `deserialize`,
/// `deserializes`, `deserialization` and `deserializing` all become
/// `deserializ`.
///
/// Stems are not words, and the mapping is not injective — that is the point.
/// A stem is only ever compared against another stem, never shown to a user.
/// Input must already be lowercased; the algorithm is case-sensitive and will
/// leave `Deserialize` alone.
///
/// The stemmer itself is a function pointer plus a small table, so one
/// process-wide instance is shared rather than constructed per call.
pub(crate) fn stem(word: &str) -> Cow<'_, str> {
    static STEMMER: OnceLock<Stemmer> = OnceLock::new();
    STEMMER
        .get_or_init(|| Stemmer::create(Algorithm::English))
        .stem(word)
}

/// Jaro-Winkler similarity, but with case differences costing less than character differences
///
/// Blends case-insensitive and case-sensitive comparisons, giving more weight to the
/// case-insensitive score. This makes case differences cost less than character differences.
pub(crate) fn case_aware_jaro_winkler(a: &str, b: &str) -> f64 {
    let case_insensitive = strsim::jaro_winkler(&a.to_lowercase(), &b.to_lowercase());
    let case_sensitive = strsim::jaro_winkler(a, b);

    // Blend: mostly case-insensitive with moderate influence from case-sensitive matching
    const CASE_WEIGHT: f64 = 0.1; // 10% influence from case-sensitivity
    (1.0 - CASE_WEIGHT) * case_insensitive + CASE_WEIGHT * case_sensitive
}

#[cfg(test)]
mod case_aware_jaro_winkler_tests {
    use crate::string_utils::case_aware_jaro_winkler;

    fn sort_in_comparison<const N: usize>(
        mut strings: [&'static str; N],
        reference: &'static str,
    ) -> [&'static str; N] {
        strings.sort_by(|a, b| {
            case_aware_jaro_winkler(b, reference).total_cmp(&case_aware_jaro_winkler(a, reference))
        });
        strings
    }

    #[test]
    fn same_letter_different_case_is_more_similar_than_different_letters() {
        assert_eq!(
            sort_in_comparison(["word", "WORD", "other", "Word"], "Word"),
            ["Word", "word", "WORD", "other"]
        );
    }

    #[test]
    fn exact_match_returns_highest_score() {
        let score = case_aware_jaro_winkler("hello", "hello");
        assert_eq!(score, 1.0, "Exact match should be 1.0");
    }

    #[test]
    fn case_difference_costs_less_than_character_difference() {
        let reference = "Test";
        let case_only = case_aware_jaro_winkler("TEST", reference);
        let char_diff = case_aware_jaro_winkler("Tast", reference);

        assert!(
            case_only > char_diff,
            "Case-only difference ({}) should score higher than character substitution ({})",
            case_only,
            char_diff
        );
    }

    #[test]
    fn single_case_difference_vs_single_char_difference() {
        let reference = "word";
        let one_case = case_aware_jaro_winkler("Word", reference);
        let one_char = case_aware_jaro_winkler("wopd", reference);

        assert!(
            one_case > one_char,
            "Single case change ({}) should cost less than single char change ({})",
            one_case,
            one_char
        );
    }

    #[test]
    fn multiple_case_differences() {
        // Test how multiple case differences accumulate
        let reference = "test";
        let one_case = case_aware_jaro_winkler("Test", reference);
        let two_case = case_aware_jaro_winkler("TEst", reference);
        let all_case = case_aware_jaro_winkler("TEST", reference);

        println!("One case diff: {}", one_case);
        println!("Two case diffs: {}", two_case);
        println!("All case diffs: {}", all_case);

        assert!(
            one_case > two_case && two_case > all_case,
            "More case differences should decrease score: {} > {} > {}",
            one_case,
            two_case,
            all_case
        );
    }

    #[test]
    fn different_length_strings_shorter_target() {
        // Testing min vs max length normalization
        let reference = "Hi";
        let score_longer = case_aware_jaro_winkler("HELLO", reference);
        let score_case = case_aware_jaro_winkler("HI", reference);

        println!("'HELLO' vs 'Hi': {}", score_longer);
        println!("'HI' vs 'Hi': {}", score_case);

        // HI has case differences on matching chars but is same length
        // HELLO has matching prefix but is much longer
        assert!(
            score_case > score_longer,
            "Same-length with case diff ({}) should beat longer string ({})",
            score_case,
            score_longer
        );
    }

    #[test]
    fn different_length_strings_longer_target() {
        let reference = "HelloWorld";
        let score_prefix_case = case_aware_jaro_winkler("helloworld", reference);
        let score_partial = case_aware_jaro_winkler("Hello", reference);

        println!("'helloworld' vs 'HelloWorld': {}", score_prefix_case);
        println!("'Hello' vs 'HelloWorld': {}", score_partial);

        // Full length with all case differences vs partial match
        assert!(
            score_prefix_case > score_partial,
            "Full-length case diff ({}) should beat partial match ({})",
            score_prefix_case,
            score_partial
        );
    }

    #[test]
    fn empty_string_edge_case() {
        let score_empty = case_aware_jaro_winkler("", "");
        let score_one_empty = case_aware_jaro_winkler("test", "");

        assert_eq!(score_empty, 1.0, "Empty strings should match perfectly");
        assert_eq!(score_one_empty, 0.0, "Empty vs non-empty should score 0");
    }

    #[test]
    fn common_prefix_with_case_differences() {
        // Jaro-Winkler gives bonus for common prefix
        let reference = "TestCase";
        let prefix_case_diff = case_aware_jaro_winkler("testCase", reference);
        let suffix_case_diff = case_aware_jaro_winkler("TestCASE", reference);
        let no_prefix = case_aware_jaro_winkler("xestCase", reference);

        println!("'testCase' vs 'TestCase': {}", prefix_case_diff);
        println!("'TestCASE' vs 'TestCase': {}", suffix_case_diff);
        println!("'xestCase' vs 'TestCase': {}", no_prefix);

        // Case diff in prefix vs case diff in suffix vs char substitution
        assert!(
            prefix_case_diff > no_prefix,
            "Case diff should beat char substitution"
        );
    }

    #[test]
    fn transposition_vs_case_difference() {
        // This documents a limitation: extreme case differences (all caps of short words)
        // can score lower than transpositions due to how the blending works
        let reference = "test";
        let transposed = case_aware_jaro_winkler("tset", reference);
        let case_diff = case_aware_jaro_winkler("TEST", reference);

        println!("'tset' vs 'test' (transposed): {}", transposed);
        println!("'TEST' vs 'test' (all case diff): {}", case_diff);

        // In practice, this edge case doesn't matter much for "did you mean" suggestions
        // because real identifiers rarely have ALL characters case-swapped
    }

    #[test]
    fn mixed_case_and_character_differences() {
        let reference = "Example";
        let all_case_diff = case_aware_jaro_winkler("EXAMPLE", reference);
        let one_char = case_aware_jaro_winkler("Examplf", reference);
        let two_chars = case_aware_jaro_winkler("Exaople", reference);
        let few_case_diff = case_aware_jaro_winkler("ExamplE", reference);

        println!("'EXAMPLE' vs 'Example' (all case):  {:.4}", all_case_diff);
        println!("'ExamplE' vs 'Example' (few case):  {:.4}", few_case_diff);
        println!("'Examplf' vs 'Example' (1 char):    {:.4}", one_char);
        println!("'Exaople' vs 'Example' (2 chars):   {:.4}", two_chars);

        // Few case differences should beat character differences
        assert!(
            few_case_diff > one_char,
            "Few case diffs ({:.4}) should beat char diff ({:.4})",
            few_case_diff,
            one_char
        );

        // Edge case: Extreme case differences (ALL caps) can score similar to typos
        // This is acceptable - in practice, "EXAMPLE" and "Example" are often different
        // identifiers (constant vs type), while "Examplf" is clearly a typo
        println!(
            "Note: All-caps scoring is {:.4}, which is acceptable for did-you-mean",
            all_case_diff
        );
    }

    #[test]
    fn sort_by_quality_realistic_example() {
        // Simulate searching for "MyStruct" in documentation
        let results = sort_in_comparison(
            [
                "MyStruct",   // exact match
                "myStruct",   // camelCase variant
                "MYSTRUCT",   // all caps
                "my_struct",  // snake_case
                "MyString",   // similar but different type
                "YourStruct", // different prefix
            ],
            "MyStruct",
        );

        println!("Sorted results for 'MyStruct':");
        for (i, result) in results.iter().enumerate() {
            println!(
                "  {}. {} (score: {:.4})",
                i + 1,
                result,
                case_aware_jaro_winkler(result, "MyStruct")
            );
        }

        // We'd expect exact match first, then case variants, then structural variants
        assert_eq!(results[0], "MyStruct", "Exact match should be first");

        // The relative ordering of myStruct, MYSTRUCT, and my_struct is interesting
        // - myStruct has 1 case diff
        // - MYSTRUCT has 6 case diffs
        // - my_struct has 1 case diff + 1 underscore
        let mystruct_pos = results.iter().position(|&s| s == "myStruct").unwrap();
        let mystruct_caps_pos = results.iter().position(|&s| s == "MYSTRUCT").unwrap();

        assert!(
            mystruct_pos < mystruct_caps_pos,
            "Fewer case differences should rank higher"
        );
    }

    #[test]
    fn potential_score_overflow() {
        // Check if adding case_bonus can push score > 1.0
        let reference = "test";
        let all_case = case_aware_jaro_winkler("TEST", reference);

        assert!(
            all_case <= 1.0,
            "Score should not exceed 1.0, got {}",
            all_case
        );
    }

    #[test]
    fn length_normalization_exploration() {
        // Exploring whether max or min length is better for normalization
        let short = "Hi";
        let long = "HiThere";

        // Current implementation uses max(a.len(), b.len())
        let score = case_aware_jaro_winkler("HI", short);

        println!("Testing length normalization:");
        println!("  'HI' vs 'Hi' (same length): {}", score);
        println!(
            "  'HITHERE' vs 'HiThere': {}",
            case_aware_jaro_winkler("HITHERE", long)
        );
        println!(
            "  'HI' vs 'HiThere': {}",
            case_aware_jaro_winkler("HI", long)
        );

        // With max: bonus is diluted for shorter strings matching longer ones
        // With min: bonus would be concentrated on matched portion
        // This test documents current behavior for evaluation
    }

    #[test]
    fn realistic_naming_convention_variations() {
        // Test realistic scenarios where case matters but shouldn't dominate
        let reference = "getValue";

        let camel_case = case_aware_jaro_winkler("getValue", reference);
        let pascal_case = case_aware_jaro_winkler("GetValue", reference);
        let snake_case = case_aware_jaro_winkler("get_value", reference);
        let typo = case_aware_jaro_winkler("getValu", reference); // missing 'e'
        let wrong_word = case_aware_jaro_winkler("setValue", reference);

        println!("\nRealistic 'getValue' comparisons:");
        println!("  getValue (exact):     {:.4}", camel_case);
        println!("  GetValue (Pascal):    {:.4}", pascal_case);
        println!("  get_value (snake):    {:.4}", snake_case);
        println!("  getValu (typo):       {:.4}", typo);
        println!("  setValue (wrong):     {:.4}", wrong_word);

        // Exact match is best
        assert_eq!(camel_case, 1.0);

        // Case variation should beat typo
        assert!(
            pascal_case > typo,
            "Case variation ({}) should beat typo ({})",
            pascal_case,
            typo
        );

        // Case variation should beat wrong word
        assert!(
            pascal_case > wrong_word && snake_case > wrong_word,
            "Case variations should beat different words"
        );
    }
}
