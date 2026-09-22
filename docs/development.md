# Development

## Prerequisites

- Rust stable (pinned by `rust-toolchain.toml`; `rustup` installs it on first build).
- Cisco Packet Tracer 9 only for the live E2E suite.

## Everyday commands

| Command | What it runs |
|---|---|
| `make` | Lists every target. |
| `make check` | `fmt-check`, `lint` and `test`. Run it before pushing; CI runs the same. |
| `make test` | Unit tests plus E2E tests against the in-process fake Packet Tracer. |
| `make lint` | Clippy with the pedantic group, warnings as errors. |
| `make e2e-live` | E2E tests against a real Packet Tracer. |
| `make release` | Optimized binary at `target/release/pktctl`. |

## Test pyramid

| Level | Where | Needs Packet Tracer |
|---|---|---|
| Unit | `#[cfg(test)]` modules next to the code | No |
| Protocol E2E | `crates/ptmp/tests/session.rs`, real TCP against `FakePt` | No |
| MCP E2E | `crates/pktctl/tests/mcp_stdio.rs`, spawns the binary and speaks JSON-RPC | No |
| Live E2E | `#[ignore]` tests in `crates/ptmp/tests/live.rs` and `crates/pktctl/tests/live.rs` | Yes |

Unit and fake-backed tests use byte sequences captured from a real Packet Tracer
9.0.1 session, so they pin the exact wire format. Feature tests run on
`pktctl::testing::Canvas`, an in-memory Packet Tracer that rejects wrongly
typed arguments exactly like the real one (`Invalid arguments for IPC call`),
so an encoding mistake fails a test instead of a live session.

When the live suite or a manual check disagrees with the canvas, fix the
canvas first so the mismatch stays covered.

## Running the live suite

1. Open Packet Tracer and register the ExApp
   ([features/exapp-registration.md](features/exapp-registration.md)).
2. Export the credentials and run:

```bash
export PKTCTL_APP_ID=dev.pktctl
export PKTCTL_SECRET='the KEY from your registration'
export PKTCTL_TEST_PKT=~/lab.pkt   # optional: any .pkt saved by Packet Tracer
make e2e-live
```

What the live suite does to the open network:

- `ptmp`: creates one router with a non-ASCII name, reads it back and deletes
  it, to prove that PTMP length prefixes count UTF-8 bytes.
- `builds_a_working_lan_using_only_tools`: builds a LAN through the MCP binary
  (router with an HWIC-2T, switch, two PCs, three cables), configures it with
  `configure_ios` and `configure_host`, pings PC to gateway, PC to PC and router
  to PC, adds a note and takes a screenshot. Every device is named `E2E-*` and
  removed at the end; leftovers from an interrupted run are removed first.
- `pktfile`: with `PKTCTL_TEST_PKT` set, decodes that file and re-encodes it,
  which is the only test that needs a file saved by Packet Tracer. This
  repository ships none, so the two tests in `crates/pktfile/tests/real_files.rs`
  fail without the variable.
- `files_round_trip_without_dialogs`: saves the open network to a temporary
  file, clears the canvas, saves and reopens a one-router network, then opens
  the saved network again. It needs the `FILE` privilege, which the pktctl
  template grants.

Run a single test with `cargo test -p pktctl --test live -- --ignored <name>`.

## Debugging

- `PKTCTL_LOG=debug` prints connection and protocol diagnostics to stderr
  (stdout is reserved for MCP).
- To watch raw PTMP traffic, put a TCP proxy between pktctl and port 39000 and
  point `PKTCTL_ADDR` at it.

## Git flow

- `main` only receives merges. Never commit to it directly.
- Branch per change: `feat/…`, `fix/…`, `docs/…`, `chore/…`.
- Conventional commit messages, atomic commits.
- Merge with `git merge --no-ff` so each branch stays visible in history.

## Building on Linux

`screenshot` captures the Packet Tracer window with
[xcap](https://crates.io/crates/xcap), which links against the desktop
libraries. On Debian and Ubuntu:

```bash
sudo apt-get install -y libxcb1-dev libxrandr-dev libdbus-1-dev \
    libwayland-dev libxkbcommon-dev pkg-config
```

macOS and Windows need nothing extra.

## Continuous integration

`make check` is what CI runs. Both pipelines are kept in step:
[.github/workflows/ci.yml](../.github/workflows/ci.yml) runs it on Linux, macOS
and Windows plus a build on the MSRV (1.88), and
[.gitlab-ci.yml](../.gitlab-ci.yml) runs the same jobs on Linux. The live suite
is not in CI: it needs a running Packet Tracer.

## Releasing

The three crates share one version, set in `[workspace.package]`.

1. Bump `version` in `Cargo.toml` and move the `Unreleased` entries of
   [CHANGELOG.md](../CHANGELOG.md) into a section for the new version.
2. `make check`, then the live suite against a real Packet Tracer.
3. Commit on a branch, merge with `--no-ff`, and tag the merge `vX.Y.Z`.
4. To publish on crates.io, publish in dependency order: `ptmp`, then
   `pktfile`, then `pktctl`. Check first with
   `cargo package -p <crate> --list` that nothing from a Packet Tracer
   installation slipped into the package.

## What must never ship

- Packet Tracer code, `.pkt` files or documentation. The IPC index is
  signatures only; an index built with `--with-summaries` carries Cisco's
  Javadoc prose and stays on your machine.
- Credentials. `PKTCTL_SECRET` belongs in the environment, never in a file
  under version control.
