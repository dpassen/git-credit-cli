#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CommitInfo {
    pub(crate) author_name: String,
    pub(crate) author_email: String,
    pub(crate) message: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Contributor {
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) commits: u64,
}
