#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| rdpv_core::fuzzing::clipboard_text(data));
