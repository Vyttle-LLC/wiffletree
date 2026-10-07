# Contributing

Thanks for helping with Wiffletree. It is early software: expect the product and its internals to change quickly.

## Before you start

- Read the [README](README.md), the [product requirements](docs/PRD.md) and [development notes](docs/DEVELOPMENT.md). The PRD is the source of truth for scope.
- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Agent contributors should also follow [AGENTS.md](AGENTS.md).

## Development

You need macOS on Apple Silicon, a recent stable Rust toolchain and Xcode with its Metal toolchain. Build and open a local bundle with:

```sh
bash scripts/package_macos.sh
open target/Wiffletree.app
```

Local bundles are ad-hoc signed and never update themselves. Use `--data-dir` with a scratch directory when you want to keep your real store untouched.

## Pull requests

- Keep each change focused, and update the PRD and docs when behavior changes.
- Run the [validation checks](docs/DEVELOPMENT.md#validation) before opening a PR: formatting, Clippy with warnings as errors, the Rust tests and the deterministic communication tests. CI runs the same checks on every PR, and they must pass before merging.
- Add tests for meaningful failure modes. The live provider smoke tests are optional and make real model calls.
- Merging does not release. Maintainers publish a release by pushing a version tag on `main`; installed copies then pick it up automatically.

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
