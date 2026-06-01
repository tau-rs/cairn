## Summary

<short summary of the change>

## Test plan

- [ ] Local gate is green:
      `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets --locked -- -D warnings`,
      `cargo test --workspace --locked`.
- [ ] If CI behavior changes, the workflow file is updated and validated.

## ADR check

- [ ] This change does not need an ADR, OR
- [ ] An ADR has been filed in `docs/decisions/` and is referenced here.
