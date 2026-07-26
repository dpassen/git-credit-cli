# git-credit

![git-credit banner](https://raw.githubusercontent.com/dpassen/git-credit-cli/main/assets/git-credit-banner.svg)

[![crates.io](https://img.shields.io/crates/v/git-credit-cli.svg)](https://crates.io/crates/git-credit-cli)
[![test](https://github.com/dpassen/git-credit-cli/actions/workflows/test.yaml/badge.svg)](https://github.com/dpassen/git-credit-cli/actions/workflows/test.yaml)
[![lint](https://github.com/dpassen/git-credit-cli/actions/workflows/lint.yaml/badge.svg)](https://github.com/dpassen/git-credit-cli/actions/workflows/lint.yaml)
[![format](https://github.com/dpassen/git-credit-cli/actions/workflows/format.yaml/badge.svg)](https://github.com/dpassen/git-credit-cli/actions/workflows/format.yaml)

CLI for safely adding `Co-authored-by` trailers to your latest Git commit.

`git-credit` finds contributors in repository history, lets you fuzzy-search and
select them, previews the resulting trailers, and amends `HEAD` after
confirmation.

## Installation

Install from crates.io with Cargo:

```console
cargo install git-credit-cli
```

This installs a binary named `git-credit`. Git discovers executables named
`git-*`, so both forms work:

```console
git-credit
git credit
```

## Usage

Run `git-credit` anywhere inside a Git working tree:

```console
git credit
```

The interface shows the current commit, its author and message, and contributors
found in repository history.

- Type to fuzzy-search by name or email.
- Use Up and Down to move.
- Press Space to select or deselect a contributor.
- Press Enter to continue and confirm.
- Press Esc to go back or cancel.

After confirmation, each selected contributor is added as a trailer:

```text
Co-authored-by: Alice Example <alice@example.com>
```

A successful amendment reports the old and new commit IDs:

```text
Added 2 co-authors: 0123abcd -> 4567efab
```

## Contributor discovery

Contributors come from commits reachable through `--all`. `git-credit` uses
Git's mailmap support, groups identities by normalized email address, and ranks
them by authored commit count. The primary author of `HEAD` is excluded.

Identities must already exist in repository history. `git-credit` does not query
GitHub, GitLab, or another network service.

## Safety

`git-credit` always targets `HEAD` and refuses to run when:

- `HEAD` is unborn or detached.
- A merge, rebase, cherry-pick, revert, or sequencer operation is in progress.
- Staged changes are present.

Unstaged and untracked files are allowed. The amendment uses
`git commit --amend --only`, so those files are not included. Before amending,
`git-credit` rechecks `HEAD`, the index, and Git operation state in case anything
changed while the interface was open.

The commit tree, parents, author, message body, and existing trailers are
preserved. Normal Git hooks and signing behavior apply. Existing commit
signatures cannot be preserved because changing the message creates a new commit
object.

Cancellation does not change the repository.

## Recovery

Amending changes the commit ID. The previous commit remains available through
the reflog and is also printed in the success message. To restore it while
keeping working-tree changes, use the old ID:

```console
git reset --keep 0123abcd
```

Inspect `git reflog` first if the old ID is no longer visible in your terminal.

## Limitations

Version 0.1:

- Only amends `HEAD`; older commits are not supported.
- Requires an interactive terminal.
- Does not accept manually entered identities.
- Does not provide a non-interactive mode.
- Rejects detached `HEAD`.
- Requires UTF-8 commit messages and identities.

## License

Licensed under either of
[MIT](https://github.com/dpassen/git-credit-cli/blob/main/LICENSE-MIT) or
[Apache-2.0](https://github.com/dpassen/git-credit-cli/blob/main/LICENSE-APACHE)
at your option.
