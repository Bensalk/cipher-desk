# ironrdp-server (Cipher Desk copy)

A copy of [ironrdp-server 0.13.0](https://crates.io/crates/ironrdp-server) from
[Devolutions IronRDP](https://github.com/Devolutions/IronRDP) (MIT OR Apache-2.0,
see `LICENSE-MIT` and `LICENSE-APACHE`). It runs the "Share screen" Host.

Why a copy: the published 0.13.0 needs the IronRDP 0.10 crates, while the rest of
Cipher Desk uses the 0.12 crates, and both cannot be in one program. Remove this
folder and use the crates.io release once one is published for the 0.12 crates.

Changes from 0.13.0:

- `Cargo.toml`: IronRDP 0.12 crate family; optional helpers removed (their
  features are kept as empty names so the source still builds). The lossless
  `qoi` / `qoiz` codecs are kept, with the same libraries as the viewer side.
- `encoder/rfx.rs`, `encoder/mod.rs`, `server.rs`: RemoteFX quantisation
  follows the viewer's quality setting (`RfxQuality`): sharpest values for a
  32-bit colour viewer, coarser ones for a viewer that asks for 16-bit colour.
  Upstream always uses the default values.
- `server.rs`: `set_honor_client_desktop_size` takes an optional size in
  acceptor 0.12; `serve()` added so the caller binds the port itself; CredSSP
  uses a network client that refuses all requests (no Kerberos, so no HTTP
  client is needed).
- `server.rs`: when the display handler refuses a viewer (`updates()` fails,
  e.g. "ask to connect" was declined), the server first sends
  `ServerSetErrorInfo(ServerDeniedConnection)` as a Share Data PDU, so the
  viewer reports a refusal instead of a lost connection and does not
  reconnect by itself.
- `server.rs`, `lib.rs`: `CookieCheck` / `set_cookie_check`: after the
  password (CredSSP) is accepted, the client's Client Info cookie is checked
  (two-step verification); a refused client gets
  `ServerSetErrorInfo(ServerFreshCredentialsRequired)` and is closed.
- `server.rs`: `ConnectionHandler::on_accept` also receives the accepted
  connection's own local address (`stream.local_addr()`), not just the
  listener's (which may be bound to all interfaces). Lets a handler check
  which network interface a connection actually arrived on, not only what
  address the peer claims — see `rust/host/src/limiter.rs`
  `local_address_allowed` and `docs/THREAT_MODEL.md` H7.
- `server.rs`: input the client sends while the connection is still being
  finalised (the acceptor's input backlog) is dropped instead of applied.
  Upstream applies it before `client_loop` asks the display handler for
  updates, which is where "ask to connect" waits for the person at the
  computer, so an unapproved viewer could type on the computer.
  `scripts/check-policy.sh` fails if the backlog handling comes back.

- `server.rs`: a new connection must send its first bytes within 10 seconds
  (`FIRST_BYTES_TIMEOUT`); one that closes at once is dropped quietly, one that
  stays silent is closed and reported to the connection handler with
  `SILENT_CONNECTION` (exported), so the lockout counts it. Connections are
  served one at a time, so a silent one used to block everyone for 30 seconds.

`scripts/check-vendored-ironrdp.sh` (run in CI on every push/PR) downloads the
real 0.13.0 release from crates.io and fails if any file in `src/` differs
from it other than the four listed above — so an unreviewed change to this
copy, or a file quietly added or removed, gets caught instead of sitting here
unnoticed. Changing which files are expected to differ, or re-basing onto a
newer upstream release, means updating both this list and the script's own
`EXPECTED_CHANGED` / `UPSTREAM_VERSION` / `EXPECTED_SHA256` together.
