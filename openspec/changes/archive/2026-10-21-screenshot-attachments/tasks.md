## 1. Desktop

- [x] 1.1 Add `staging.rs`: open (empty at start), stage a dropped file, save a pasted image as PNG, release a copy, name pasted images.
- [x] 1.2 Stage drops and pastes before `InspectAttachments`; merge the results with `add_staged`, which releases copies left out.
- [x] 1.3 Release copies when a draft is sent or a file is removed; take image and file pastes through `Textarea::on_paste`.

## 2. Docs

- [x] 2.1 Update the Attachments section and the known limits in `docs/DEVELOPMENT.md`.

## 3. Verify and archive

- [x] 3.1 Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace`; drive a drop and a paste with `scripts/drive_desktop.py`.
- [x] 3.2 Run `openspec archive screenshot-attachments`, then `openspec validate --all --strict`.
