use super::*;
#[test]
fn changed_encoding_does_not_imply_changed_content() {
    assert_eq!(
        state(b"packed-a", b"packed-b", b"text", b"text"),
        "compression_only"
    );
    assert_eq!(
        state(b"packed-a", b"packed-b", b"text", b"other"),
        "content_changed"
    );
    assert_eq!(state(b"packed-a", b"packed-a", b"text", b"text"), "same");
}
