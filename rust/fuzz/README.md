# Fuzzers (SA-3)

Coverage-guided fuzzing (libFuzzer) of **our own** code that reads untrusted input.
IronRDP's protocol parsers are fuzzed upstream (`ironrdp-fuzzing`).

| Target | Input comes from | Checks |
|--------|------------------|--------|
| `server_text` | remote computer (error and disconnect texts) | always mapped to a fixed catalogue error |
| `clipboard_text` | remote computer (clipboard bytes) | decodes safely; no NUL kept; size accounting |
| `certificate` | remote computer (TLS certificate) | key extraction and fingerprint never panic |
| `desktop_size` | remote computer (desktop size) | only sizes within the 8K limit are accepted |
| `files` | app data / imported files | connections, import, settings and pin files: limits respected, only valid entries kept, imports never enable clipboard |
| `form_fields` | the user | addresses, usernames, names, ports and MAC addresses parse safely and round-trip |

Run (stable Rust; needs a C++ compiler for libFuzzer):

```sh
bash rust/fuzz/run.sh 60            # 60 s per target
bash rust/fuzz/run.sh 600 files     # one target for 10 minutes
```

`seeds/` holds the starting inputs (checked in; addresses use reserved examples
only). New interesting inputs go to `corpus/` and crashing ones to `artifacts/`
(both ignored by git). The crate is a separate workspace, never part of the app,
and uses the core's `fuzzing` feature, which the app never enables.
