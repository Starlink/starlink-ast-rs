# Releasing a crate

The published crates are `starlink-ast` and, once it exists, `starlink-ast-capi`.
Each crate is versioned on its own, and each release is one commit on `main` and one tag, `<crate>-v<version>`, for example `starlink-ast-v0.1.0`.

1. Move the crate's `Unreleased` entries in `CHANGELOG.md` under a new `## [<version>] - <YYYY-MM-DD>` heading, leave an empty `Unreleased` section above it, and add the version's compare link at the bottom of the file.
2. Decide the version with `cargo-semver-checks`, which compares the public API against the last version published on crates.io:

   ```shell
   cargo install cargo-semver-checks --locked   # once
   cargo semver-checks check-release -p starlink-ast
   ```

   It lists every breaking change it finds and the bump it requires.
   A reported break is either a bug to fix or the reason `y` rises in `0.y.z`; it is never silenced.
   There is nothing to compare against before the first release, so the step starts with the second.
3. Set `version` in the crate's `Cargo.toml`.
4. Build the archive in isolation, which is what docs.rs and every user will build:

   ```shell
   cargo package -p starlink-ast
   ```

   The archive carries the subset of `tests/fixtures/` that `tests/fixtures/DIST_MANIFEST` names, as the C library's default `make dist` does: enough to show that a build works, without the corpora that exist to catch library defects.
   `cargo test` from the unpacked archive reports the tests that need the rest as ignored; CI's `package` job runs exactly that on every pull request, and `tests/test_dist_manifest.rs` fails if the manifest and the `include` list in `Cargo.toml` differ.

5. Run the full suite, `cargo test -p starlink-ast --no-fail-fast`, and the tier 3 and 4 checks when the submodule is present.
6. Commit as `starlink-ast: release <version>`, tag the commit `starlink-ast-v<version>`, and push both.
7. Pushing the tag runs `.github/workflows/release.yml`, which runs the CI workflow on the tagged commit and, if it passes, publishes the crate.
   It authenticates with crates.io Trusted Publishing, exchanging a GitHub OIDC token for a short-lived publish token, so no crates.io token is stored in the repository.
   It refuses a tag whose version differs from the crate's `Cargo.toml`, and does nothing if that version is already on crates.io.

   Trusted Publishing cannot create a crate, so the first release of each crate is published by hand once:

   1. On crates.io, verify your email address under Account Settings, Profile; publishing is refused without one.
   2. Under Account Settings, API Tokens, create a token with only the `publish-new` scope, restricted to the crate name, with a short expiry.
   3. From the release commit, run

      ```shell
      CARGO_REGISTRY_TOKEN=<token> cargo publish -p starlink-ast --locked
      ```

   4. On the crate's page, open Settings, Trusted Publishing, and add a GitHub publisher with owner `Starlink`, repository `starlink-ast-rs` and workflow `release.yml`, leaving the environment blank.
      The publisher is tied to the repository's owner and name, so a transfer or rename must be followed by updating it.
   5. Revoke the token.

   Add owners on the crate's Settings page as soon as the crate exists, so nothing depends on one account.
   A GitHub team owner (`github:Starlink:<team>`) lets every member publish and yank, but team owners cannot add or remove owners or change Trusted Publishing, so at least one other person must be a user owner as well.
   A user owner must have logged into crates.io once and accepts an invitation; adding a team requires being a member of it and crates.io having the `read:org` GitHub scope.
8. Create the GitHub release from the tag, with the version's changelog section as its notes.

After the first release, add the `cargo-semver-checks` GitHub Action to CI so every pull request is checked against the latest published version rather than only at release time.
