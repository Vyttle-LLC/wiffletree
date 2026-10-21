## 1. Host

- [x] 1.1 Yield candidate URLs for `owner/name` in gh's protocol order, or the named URL alone, and keep the first `git ls-remote` reads; name each failure in one error.
- [x] 1.2 Pass gh's credential helper with `-c` to every Git command that reaches an HTTPS `github.com` settings repository: the check, clone, fetch and push.
- [x] 1.3 Dispatch the default `Tools` Git commands by the command after any `-c` options.

## 2. Tests and docs

- [x] 2.1 Unit-test candidate order, the fallback, a named URL, the combined error and gh's login on later Git calls over HTTPS, and none over SSH.
- [x] 2.2 Describe the fallback and the helper in `docs/DEVELOPMENT.md`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`; run a read-only `ls-remote` and clone against a real private repository.
- [x] 3.2 Run `openspec archive settings-repository-gh-login`, then `openspec validate --all --strict`.
