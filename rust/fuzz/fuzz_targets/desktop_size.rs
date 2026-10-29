#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| rdpv_core::fuzzing::desktop_size(data));
