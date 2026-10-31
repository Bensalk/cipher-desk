# ironrdp-server (Cipher Desk copy)

A copy of [ironrdp-server 0.13.0](https://crates.io/crates/ironrdp-server) from
[Devolutions IronRDP](https://github.com/Devolutions/IronRDP) (MIT OR Apache-2.0,
see `LICENSE-MIT` and `LICENSE-APACHE`). It runs the "Share screen" Host.

Why a copy: the published 0.13.0 needs the IronRDP 0.10 crates, while the rest of
Cipher Desk uses the 0.12 crates, and both cannot be in one program. Remove this
folder and use the crates.io release once one is published for the 0.12 crates.

Changes from 0.13.0:

- `Cargo.toml`: IronRDP 0.12 crate family; optional codecs and helpers removed
  (their features are kept as empty names so the source still builds).
- `server.rs`: `set_honor_client_desktop_size` takes an optional size in
  acceptor 0.12; `serve()` added so the caller binds the port itself; CredSSP
  uses a network client that refuses all requests (no Kerberos, so no HTTP
  client is needed).
