# Contributing to Smooblue

Thanks for your interest! Smooblue is a young project — most contributions
land quickly if they're small and focused.

## Getting set up

```bash
git clone https://github.com/SmooAI/smooblue.git
cd smooblue
cargo test --workspace
cargo run --bin smooblue-app
```

You'll need:

- Rust **1.80+** (use [rustup](https://rustup.rs/))
- [`librsvg`](https://wiki.gnome.org/Projects/LibRsvg) only if regenerating icons
- A Bluesky account for end-to-end testing

## Workflow

1. Open an issue describing what you want to change. For non-trivial
   work this avoids duplicate effort.
2. Fork + branch from `main`: `git checkout -b your-feature-name`.
3. Write code + tests. **New crates / public functions must have tests.**
4. Run the full quality gate:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --tests -- -D warnings
   cargo test --workspace
   ```

5. Open a PR. CI will re-run the gate and the real-Bluesky integration
   tests on a nightly schedule.

## What's in scope

- **Yes**: new column types, post composition, media renderers, UX polish,
  performance, accessibility, packaging (Linux/Windows/macOS bundles).
- **Maybe**: alternative ATproto AppViews, federation features, advanced
  moderation tooling. Open an issue first to discuss.
- **No**: anything that changes the auth model away from OAuth+DPoP. App
  passwords aren't supported and won't be.

## Code style

- `cargo fmt --all` (rustfmt defaults)
- `cargo clippy -- -D warnings` (no lint warnings)
- Prefer small, focused PRs over big ones.
- Tests live next to the code they cover (`#[cfg(test)] mod tests`) for
  unit tests, in `tests/` for integration tests.
- For HTTP-mocked tests, use [`wiremock`](https://crates.io/crates/wiremock).

## Reporting bugs / asking questions

- **Bug**: open an issue with a minimal repro + your OS + `rustc --version`.
- **Feature idea**: open an issue with the use case.
- **Security**: please email security@smoo.ai instead of filing publicly.

## License

By contributing, you agree your work is licensed under the [MIT License](LICENSE).
