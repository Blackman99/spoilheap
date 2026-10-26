# Contributing

Thanks for helping. spoilheap deletes files on people's machines, so the bar is simple: **never delete something a person might want back without asking, and never act outside an agent-owned folder.**

## Before you open a PR
- Read [GOAL.md](GOAL.md). PRs outside it are closed with a pointer there.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass. CI runs them on Linux, macOS and Windows.
- **New paths:** link the agent's source or docs that define the folder, in the PR and in a code comment. Add it to `spoilheap paths`, to the coverage table in the README and to the docs.
- **New process matchers (`procs`):** match an agent's own executable, package or marker variable, never a generic runtime like `node` or `python`. Add a fixture in `tests/fixtures/procs/` with the real command line and a test for its verdict.
- **New verdict rules:** add a fixture to `scripts/demohome.sh` or `tests/` that shows the rule both ways (for example SAFE when merged, REVIEW when not), and an assertion in the CI `demo-home` job.
- **Anything that deletes:** it must go through `clean`'s re-check, stay inside the item's own folder, never follow symlinks, and print what it will do in the dry run.

## Running it on a fake home
```sh
cargo build --release
scripts/demohome.sh /tmp/h          # small files
scripts/demohome.sh /tmp/h big      # realistic sizes as sparse files
SPOILHEAP_APPARENT_SIZE=1 target/release/spoilheap --home /tmp/h --root /tmp/h/code
```
`SPOILHEAP_FAKE_FREE=3GB/500GB` simulates a nearly full disk for `guard` and `watch`.

## Re-recording the demo and images
- `docs/demo.gif`: `vhs docs/demo.tape` (see the comment at the top of the tape).
- `site/og.png` and `site/social-preview.png`: `scripts/share/render.sh`.

## Reporting a security problem
Please do not open a public issue for anything that could delete the wrong files. Use GitHub's private vulnerability reporting on this repository.
