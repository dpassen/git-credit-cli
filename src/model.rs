use std::{
    cmp::Ordering,
    collections::{HashMap, hash_map::Entry},
};

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

struct ContributorGroup {
    contributor: Contributor,
    preferred_identity_commits: u64,
}

pub(crate) fn consolidate_contributors(
    identities: impl IntoIterator<Item = Contributor>,
) -> Vec<Contributor> {
    let mut identity_counts: HashMap<(String, String), u64> = HashMap::new();
    for identity in identities {
        *identity_counts
            .entry((identity.name, identity.email))
            .or_default() += identity.commits;
    }

    let mut groups: HashMap<String, ContributorGroup> = HashMap::new();
    for ((name, email), commits) in identity_counts {
        let identity = Contributor {
            name,
            email,
            commits,
        };
        let normalized_email = identity.email.to_ascii_lowercase();
        match groups.entry(normalized_email) {
            Entry::Occupied(mut entry) => entry.get_mut().add(identity),
            Entry::Vacant(entry) => {
                let preferred_identity_commits = identity.commits;
                entry.insert(ContributorGroup {
                    contributor: identity,
                    preferred_identity_commits,
                });
            }
        }
    }

    let mut contributors: Vec<_> = groups
        .into_values()
        .map(|group| group.contributor)
        .collect();
    contributors.sort_by(contributor_order);
    contributors
}

impl ContributorGroup {
    fn add(&mut self, identity: Contributor) {
        let is_preferred = identity.commits > self.preferred_identity_commits
            || (identity.commits == self.preferred_identity_commits
                && (&identity.name, &identity.email)
                    < (&self.contributor.name, &self.contributor.email));

        self.contributor.commits += identity.commits;

        if is_preferred {
            self.contributor.name = identity.name;
            self.contributor.email = identity.email;
            self.preferred_identity_commits = identity.commits;
        }
    }
}

fn contributor_order(left: &Contributor, right: &Contributor) -> Ordering {
    right
        .commits
        .cmp(&left.commits)
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.email.cmp(&right.email))
}
