# Vendored crates

## gpui-component 0.5.1

An unmodified copy of the crates.io release, plus one backport: [longbridge/gpui-kit#2093](https://github.com/longbridge/gpui-kit/pull/2093), in `render_root` in `src/text/node.rs`. Without it, a non-scrollable `TextView` gives every Markdown block zero bottom margin, so paragraphs, lists and headings run together.

The fix shipped only in 0.6 and later, which require a different gpui fork. Delete this directory and the `[patch.crates-io]` entry in the root `Cargo.toml` when the desktop app moves to one of those releases.
