# Homebrew packaging

Naskah installs from the `iman1704/tap` tap:

```sh
brew tap iman1704/tap
brew install iman1704/tap/naskah
```

The tap's `Formula/naskah.rb` installs a prebuilt binary; it does not compile
Rust on the user's machine. The formula is generated from
`naskah.rb.tmpl` by `scripts/render-homebrew-formula.sh` and pushed to
`iman1704/homebrew-tap` by the release workflow.

## Release assets

A `v*` tag builds four tarballs, each containing the `naskah` binary, `README.md`,
and `LICENSE`:

| Asset | Platform |
| --- | --- |
| `naskah-vX.Y.Z-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon |
| `naskah-vX.Y.Z-x86_64-apple-darwin.tar.gz` | macOS Intel |
| `naskah-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` | Linux arm64 |
| `naskah-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 |

The release also publishes `SHA256SUMS`. macOS binaries are ad-hoc signed with
`codesign --force --sign -` after stripping, which is required for Apple Silicon
and avoids an unsigned-binary failure. They are not notarized, so a binary that
has a quarantine attribute (for example after downloading a tarball in a
browser) can still be blocked by Gatekeeper. The formula's
`post_install_steps` block clears `com.apple.quarantine` from the installed
prefix, which is the formula equivalent of a cask `postflight` stanza. The
caveat keeps the manual `xattr` command as a fallback for tarball installs.

Linux binaries are built on Ubuntu 22.04, so they need glibc 2.35 or newer.
Older distributions can clone the repository and run
`cargo install --locked --path .`.

## One-time setup

These steps cannot be done from this checkout because they need GitHub access:

1. Push this repository to `https://github.com/iman1704/naskah` and enable
   Actions. The current remote `main` only contains `LICENSE`, so reconcile that
   history before the first push.
2. Create a fine-grained personal access token for `iman1704/homebrew-tap` with
   **Contents: Read and write**. Add it to `iman1704/naskah` as the repository
   secret `TAP_GITHUB_TOKEN` (Settings → Secrets and variables → Actions).
3. If `iman1704/homebrew-tap` has branch protection on `main`, allow the token
   to push (or update the formula manually; see below).

`TAP_GITHUB_TOKEN` is a PAT rather than `GITHUB_TOKEN` because the default
token cannot write to another repository.

The tap currently has an older Go-based `Formula/naskah.rb`. The first release
replaces it with the prebuilt-binary formula.

### First push

This checkout has no commits yet, and the remote `main` only has `LICENSE`. If
that is still true, adopt the remote commit before creating the first local
commit:

```sh
git remote add origin git@github.com:iman1704/naskah.git
git fetch origin
git reset --mixed origin/main   # keep the working tree, adopt the remote commit
git add -A
git commit -m "Rust rewrite"
git push -u origin main
```

Run this before creating local commits; `git reset --mixed` would move the
branch away from them.

## Releasing

1. Bump `version` in `Cargo.toml` and commit it.
2. Tag and push:

   ```sh
   git tag v0.1.0
   git push origin main
   git push origin v0.1.0
   ```

3. The `Release` workflow builds the four binaries, creates the GitHub release,
   generates the formula, and commits it to `iman1704/homebrew-tap`.
4. Verify:

   ```sh
   brew update
   brew upgrade naskah
   brew audit --strict iman1704/tap/naskah
   ```

The workflow can also be started manually with an existing tag
(`workflow_dispatch`). The tag must match the `Cargo.toml` version.

## Manual tap update

If the tap push fails, run the renderer locally and copy the result:

```sh
# Download SHA256SUMS from the GitHub release first.
scripts/render-homebrew-formula.sh 0.1.0 SHA256SUMS > /tmp/naskah.rb
cp /tmp/naskah.rb ../homebrew-tap/Formula/naskah.rb
```

Then commit and push that file in the tap repository.

## Linux arm64 runner

The workflow uses `ubuntu-22.04-arm`. GitHub-hosted arm64 Linux runners are free
for public repositories but need a paid plan for private ones. If the source
repository is private, remove that matrix entry or use a self-hosted runner.
