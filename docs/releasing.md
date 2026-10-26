# Releasing a crate

The published crates are `starlink-ast` and, once it exists, `starlink-ast-capi`.
Each crate is versioned on its own, and each release is one commit on `main` and one tag, `<crate>-v<version>`, for example `starlink-ast-v0.1.0`.
A crate's first release has extra steps, because crates.io Trusted Publishing cannot create a crate and there is no published version to compare against; they are in [The first release of a crate](#the-first-release-of-a-crate) below.

## Every release

1. Move the crate's `Unreleased` entries in `CHANGELOG.md` under a new `## [<crate> <version>] - <YYYY-MM-DD>` heading, leave an empty `Unreleased` section above it, and at the bottom of the file add the heading's link, `https://github.com/Starlink/starlink-ast-rs/releases/tag/<crate>-v<version>`, and point the `Unreleased` link at the comparison from this tag to `main`.
   The version heading names the crate because each crate has its own version sequence; the entries under it need not.
2. Decide the version with `cargo-semver-checks`, which compares the public API against the last version published on crates.io:

   ```shell
   cargo install cargo-semver-checks --locked   # once
   cargo semver-checks check-release -p starlink-ast
   ```

   It lists every breaking change it finds and the bump it requires.
   A reported break is either a bug to fix or the reason `y` rises in `0.y.z`; it is never silenced.
   It only vetoes: a bump larger than it requires passes, and it never chooses the number.
3. Set `version` in the crate's `Cargo.toml`; that field is the only place the version is recorded, and the release workflow refuses a tag that does not match it.
   Between releases the field holds the last published version, with no pre-release suffix: the size of the bump is decided by step 2 from what actually changed, so it cannot be chosen in advance.
   `Cargo.lock` records the version too and changes on the next cargo run.
4. Build the archive in isolation, which is what docs.rs and every user will build:

   ```shell
   cargo package -p starlink-ast
   ```

   `cargo package` builds the unpacked archive to check that it compiles, but runs no tests.
   The archive carries the subset of `tests/fixtures/` that `tests/fixtures/DIST_MANIFEST` names, as the C library's default `make dist` does: enough to show that a build works, without the corpora that exist to catch library defects.
   `cargo test` from the unpacked archive reports the tests that need the rest as ignored; CI's `package` job runs exactly that on every pull request, and `tests/test_dist_manifest.rs` fails if the manifest and the `include` list in `Cargo.toml` differ.
   To run it by hand, unpack `target/package/starlink-ast-<version>.crate` somewhere outside the repository and run `cargo test` there.

5. Run the full suite in the repository checkout, `cargo test -p starlink-ast --no-fail-fast`, and the tier 3 and 4 checks when the submodule is present.
   The checkout has the fixture corpora the archive leaves out, so this covers more than step 4.
6. Commit the changes of steps 1 and 3, `CHANGELOG.md`, the crate's `Cargo.toml` and `Cargo.lock`, as `starlink-ast: release <version>`.
   Merge it to `main`, tag the commit that lands on `main` (a squash or merge commit is fine) as `starlink-ast-v<version>`, and push the tag:

   ```shell
   git checkout main && git pull
   git tag starlink-ast-v<version>
   git push origin starlink-ast-v<version>
   ```

7. Pushing the tag runs `.github/workflows/release.yml`, which runs the CI workflow on the tagged commit and, if it passes, publishes the crate.
   It authenticates with crates.io Trusted Publishing, exchanging a GitHub OIDC token for a short-lived publish token, so no crates.io token is stored in the repository.
   It refuses a tag whose version differs from the crate's `Cargo.toml`, and does nothing if that version is already on crates.io, so a tag can be pushed again after a failed run.
8. Create the GitHub release from the tag, with the version's changelog section as its notes.

After the first release, add the `cargo-semver-checks` GitHub Action to CI so every pull request is checked against the latest published version rather than only at release time.

## The first release of a crate

Trusted Publishing cannot create a crate, so the first version is published by hand from the release commit, and the tag is pushed only after that, so that the workflow finds the version already on crates.io.
There is also nothing for `cargo-semver-checks` to compare against, so step 2 above is skipped.

Before the release commit:

1. Check the crate's `Cargo.toml`.
   `cargo publish` needs `description` and `license`, and docs.rs and the crates.io page use `repository`, `documentation`, `readme`, `keywords` and `categories`; the workspace supplies `license` and `repository`.
   `publish = false`, which the crates that are not published carry, must be absent.
   An `include` list says what the archive carries; `cargo package -p <crate> --list` shows the result.
2. Teach the workflows about the crate.
   The `package` job of `.github/workflows/ci.yml` builds and tests the archive of `starlink-ast` alone, and `.github/workflows/release.yml` publishes `starlink-ast` alone: its tag filter and `CRATE` name the crate.
   Extend the `package` job to the new crate, and have `release.yml` take the crate name from the tag (the part before `-v<version>`) with a tag filter that admits every crate.
3. Make sure every change the crate's users can see is under `Unreleased` in `CHANGELOG.md`, each entry naming the crate.

The release:

4. Follow steps 1 and 3 to 6 of [Every release](#every-release), stopping before the tag is pushed: the release commit is on `main`, tagged locally or not yet at all.
5. On crates.io, verify your email address under Account Settings, Profile; publishing is refused without one.
6. Under Account Settings, API Tokens, create a token with only the `publish-new` scope, restricted to the crate name, with a short expiry.
7. From the release commit on `main`, run

   ```shell
   CARGO_REGISTRY_TOKEN=<token> cargo publish -p <crate> --locked
   ```

8. On the crate's page, open Settings, Trusted Publishing, and add a GitHub publisher with owner `Starlink`, repository `starlink-ast-rs` and workflow `release.yml`, leaving the environment blank.
   The publisher is tied to the repository's owner and name, so a transfer or rename must be followed by updating it.
   Every crate is configured separately.
9. Revoke the token.
10. Add owners on the crate's Settings page as soon as the crate exists, so nothing depends on one account.
    A GitHub team owner (`github:Starlink:<team>`) lets every member publish and yank, but team owners cannot add or remove owners or change Trusted Publishing, so at least one other person must be a user owner as well.
    A user owner must have logged into crates.io once and accepts an invitation; adding a team requires being a member of it and crates.io having the `read:org` GitHub scope.
11. Tag the release commit `<crate>-v<version>` and push the tag.
    The workflow runs CI on it, finds the version on crates.io, and publishes nothing; a tag pushed before the hand publish fails instead, at the authentication step, because the publisher is not configured yet.
12. Create the GitHub release from the tag, as in step 8 of [Every release](#every-release).
