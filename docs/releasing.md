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

5. Run the full suite, `cargo test -p starlink-ast --no-fail-fast`, and the tier 3 and 4 checks when the submodule is present.
6. Commit as `starlink-ast: release <version>`, tag the commit `starlink-ast-v<version>`, and push both.
7. Publish:

   ```shell
   cargo publish -p starlink-ast
   ```

   Publishing needs a crates.io token with publish scope (`cargo login`).
   The crate's owners on crates.io should include a GitHub team (`cargo owner --add github:Starlink:<team> starlink-ast`) so a release never depends on one account.
8. Create the GitHub release from the tag, with the version's changelog section as its notes.

After the first release, add the `cargo-semver-checks` GitHub Action to CI so every pull request is checked against the latest published version rather than only at release time.
