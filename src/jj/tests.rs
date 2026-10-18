use super::{Revision, parse_revision_output, validate_target};

fn revision(description: &str) -> Revision {
    Revision {
        commit_id: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        change_id: "abcdefghijklmnopabcdefghijklmnop".to_owned(),
        is_immutable: false,
        description: description.to_owned(),
    }
}

#[test]
fn parses_revision_information() {
    let output = b"0123456789abcdef\0abcdefghijklmnop\0false\0Description\n\0";

    let parsed = parse_revision_output("@", output).expect("revision should be parsed");

    assert_eq!(
        parsed,
        Revision {
            commit_id: "0123456789abcdef".to_owned(),
            change_id: "abcdefghijklmnop".to_owned(),
            is_immutable: false,
            description: "Description\n".to_owned(),
        }
    );
}

#[test]
fn rejects_revision_sets_with_multiple_commits() {
    let output = b"one\0change-one\0false\0One\0two\0change-two\0false\0Two\0";

    let error = parse_revision_output("@-", output)
        .expect_err("multiple parent commits should be rejected");

    assert_eq!(
        error.to_string(),
        "revision @- resolved to multiple commits"
    );
}

#[test]
fn rejects_the_root_commit_and_immutable_commits() {
    let mut target = revision("Description\n");
    target.commit_id = "0000000000000000000000000000000000000000".to_owned();
    assert_eq!(
        validate_target(&target).unwrap_err().to_string(),
        "cannot add co-authors to Jujutsu's root commit"
    );

    target.commit_id = "0123456789abcdef0123456789abcdef01234567".to_owned();
    target.is_immutable = true;
    assert_eq!(
        validate_target(&target).unwrap_err().to_string(),
        "target commit is immutable"
    );
}
