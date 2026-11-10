# bimcraft-update

The updater, one of the two crates that reach the network (the other is [`bimcraft-assistant`](../assistant/CLAUDE.md)): `apps/bimcraft`'s
`main` attaches it and starts the check on a thread, and the core, the CLI and the MCP server do not depend on it (a test reads every manifest). What it takes from the network is hostile:
the reply and the download are capped, the installer's address is built from the release's tag and never read from the reply, an
asset is taken only if it is named exactly for the version and the kind of install (`Target::asset_name`), uploaded, and carries its
`sha256:` digest, and nothing runs that failed a check. The write-up of each platform's sequence (Windows hand-over, AppImage, macOS app) is
[updates](../../docs/areas/updates.md); the modules are in the root `CLAUDE.md`.

- A new check is a pure function with a test; the network stays behind `Net` (`testing::Fake` in tests; `net::Ureq` is tested
  against a server on this machine, never GitHub).
- The release workflow tests on Windows, Linux and macOS, so a test with Unix paths, a shell script, `chmod` or a link is gated
  `#[cfg(unix)]`; the script tests take the `serial()` guard against ETXTBSY. A helper only such tests call is gated too, or clippy
  on Windows stops on it as dead code.
- Rename a release asset in `.github/workflows/release.yml` and updates quietly stop installing: keep it as `Target::asset_name` names it.
- `cargo test -p bimcraft-update` runs in about five seconds.
