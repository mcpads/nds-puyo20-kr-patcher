use super::*;
#[test]
fn distinguishes_whitespace_and_arguments() {
    assert_eq!(
        script_delta(b"MzSetText\t0 1\n", b"MzSetText 0 1\n")["comparison"],
        "whitespace_tokens_same"
    );
    assert_eq!(
        script_delta(b"MzSetText 0 1", b"MzSetText 0 2")["changed_positions"],
        "{\"MzSetText:argument_2\":1}"
    );
}
#[test]
fn diff_tracks_growth_and_separated_writes() {
    assert_eq!(
        delta_ranges(b"abcde", b"AbCdex"),
        vec![(0, 1), (2, 3), (5, 6)]
    );
}
