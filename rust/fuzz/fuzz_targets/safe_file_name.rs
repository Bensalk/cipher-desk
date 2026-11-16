#![no_main]

// `safe_file_name` turns a name the *other side* of a clipboard file transfer
// sent us (host receiving from the viewer, or viewer receiving from a host)
// into a name safe to create on disk. It is the one place that stands
// between an attacker-controlled string and a real file path, so it gets
// its own fuzz target rather than relying only on the hand-picked unit
// tests in rust/core/src/xfer.rs.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Ok(raw) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(name) = rdpv_core::xfer::safe_file_name(raw) {
        // Whatever comes back must be usable as a single, plain path
        // component directly under the receive folder: no separator, no
        // way to climb out, nothing Windows itself would refuse to create.
        assert!(!name.is_empty());
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        assert_ne!(name, ".");
        assert_ne!(name, "..");
        assert!(!name.ends_with('.'));
        assert!(!name.ends_with(' '));
        assert!(name.chars().count() <= 255);
        assert!(!name
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')));
    }
});
