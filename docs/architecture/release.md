# Packaging, Release, and Commit Messages

## Packaging and Release

- Requirements: `Windows.Desktop` 10.0.19041 or later, x64 only, `runFullTrust`. Store submission
  packages are unsigned and rely on Store signing.
- `Cargo.toml` always names a version that has **not** shipped. The release workflow stamps the tag
  version with `Set-CrateVersion.ps1` before the `--locked` build, restores the files, then bumps the
  tree in the post-release commit. `winres` derives `FileVersion` from `CARGO_PKG_VERSION`, so there
  is one source of truth, and `validate-msix.ps1` compares all four numeric parts of both executables
  against the package version.
- `package-msix.ps1` stamps only the manifest and artifact names, so it fails fast when its
  `-Version` differs from the crate version — before either expensive build, rather than producing a
  package `validate-msix.ps1` will reject. Stamping stays owned by the workflow. Documentation of the
  local sequence must derive the version (`cargo metadata --no-deps`) instead of naming a literal,
  which would go stale at the next release and re-arm that failure.
- Store submission is not done, and neither is coverage on 10.0.19041 (the minimum) or Windows 11.
- Launching the installed settings UI from the tray requires a one-time user click check, because the
  process is created from inside the package; that is expected for an executable that is not an
  application entry point. The first launch can take 10–20 seconds for framework initialization;
  later launches connect in about a second.

## Commit Messages - Conventional Commits

```text
<type>(<scope>): <description>

[optional body]

[optional footer]
```

- **Types**: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `ci`, `build`, `chore`.
- **Scope**: the area touched — `core`, `timing`, `socd`, `input`, `ipc`, `ui`, `settings`,
  `measurement`, `linux`, `msix`, `packaging`, `release`. Omit only for a repository-wide change.
- **Description**: imperative mood, lowercase, no trailing period, subject within ~72 characters.
- **Body**: explain *why*, not *what*. Name the invariant or contract a change protects.
- **Breaking changes**: `!` after the scope plus a `BREAKING CHANGE:` footer. Bumping
  `PROTOCOL_VERSION` is always breaking.

The release workflow depends on this convention: it commits the version bump as
`chore(release): prepare <tag>`.
