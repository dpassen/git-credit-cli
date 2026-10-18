pub(crate) fn amendment_summary(count: usize, old_id: &str, new_id: &str) -> String {
    let label = if count == 1 {
        "co-author"
    } else {
        "co-authors"
    };

    format!(
        "Added {count} {label}: {} -> {}",
        abbreviate_id(old_id),
        abbreviate_id(new_id)
    )
}

fn abbreviate_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
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
}
