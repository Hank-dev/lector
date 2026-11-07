# Publishing and releases

## Before changing repository visibility

1. Review tracked files **and Git history**, including author email addresses.
   Decide whether the existing metadata is suitable for public disclosure.
   `.gitignore` does not remove anything already committed.
2. Run `gitleaks git --redact --log-opts=--all` and `cargo audit`. Inspect any
   findings; revoke exposed credentials before removing them from history.
3. Run the checks in [CONTRIBUTING.md](../CONTRIBUTING.md), including the
   virtual-display suite and staged installation. Check the final diff.
4. Commit and push the reviewed changes, then verify the **Arch Linux** CI job
   on GitHub. A local pass is not a hosted CI result.
5. Set the repository description to “Native GTK4 PDF reader and form filler
   with live Omarchy theming”. Suggested topics: `pdf`, `pdf-viewer`, `rust`,
   `gtk4`, `linux`, `omarchy`.
6. Make the repository public when ready. Enable private vulnerability
   reporting, Dependabot alerts, and secret scanning in repository settings
   where available. Require the CI check for pull requests to `main`.

## After removing private author information

Publish the cleaned history to a **new GitHub repository**, keeping the old
repository private. If the public project needs the old name, rename the
private repository first, then create the new repository under the public
name. Do not simply change the old repository's visibility after a force push:
[GitHub documents that cached commit views and pull-request references can
retain old history](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository).

Use the sanitized checkout for publication. Never merge old branches into it;
that would reintroduce the original author metadata. Set a repository-local
GitHub noreply email for future commits, and review all author/committer/tagger
addresses before pushing. Keep email replacement maps outside the repository.

## Cutting a release

1. Update the package version and lockfile together. Move the relevant
   changelog entries from Unreleased to the version and release date.
2. Run the full checks and confirm hosted CI passes for the exact commit.
3. Create and push an annotated tag matching `v<package-version>`.
4. Draft GitHub release notes with supported systems, install instructions,
   known limitations, and the distinction between PDF-embedded edits and
   Lector's sidecar notes. Review, then publish the draft.

Start with GitHub's source archives. A locally built executable dynamically
links to distribution libraries and must not be advertised as a portable
Linux binary. If shipping packages later, include the GPL license, matching
source, dependency requirements, and checksums; test in a clean target system.
