use super::amendment_summary;

#[test]
fn formats_amendment_summaries() {
    assert_eq!(
        amendment_summary(1, "0123456789abcdef", "fedcba9876543210"),
        "Added 1 co-author: 01234567 -> fedcba98"
    );
    assert_eq!(
        amendment_summary(2, "old", "new"),
        "Added 2 co-authors: old -> new"
    );
}
