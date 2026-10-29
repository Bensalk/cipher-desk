#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| rdpv_core::fuzzing::server_text(data));
