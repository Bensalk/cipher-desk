import 'package:flutter/services.dart';

/// A PC keyboard scancode (set 1), as sent to the remote computer.
class Scancode {
  const Scancode(this.code, {this.extended = false});
  final int code;
  final bool extended;
}

/// Maps a physical key (USB HID usage) to its PC scancode.
///
/// Physical keys are sent rather than characters, so the remote computer's own
/// keyboard layout decides which character is typed — the same behaviour as
/// Microsoft's Remote Desktop client.
Scancode? scancodeFor(PhysicalKeyboardKey key) => _map[key.usbHidUsage];

const _ext = true;

final Map<int, Scancode> _map = {
  // Letters A–Z (HID 0x04–0x1D)
  0x00070004: const Scancode(0x1E), 0x00070005: const Scancode(0x30),
  0x00070006: const Scancode(0x2E), 0x00070007: const Scancode(0x20),
  0x00070008: const Scancode(0x12), 0x00070009: const Scancode(0x21),
  0x0007000A: const Scancode(0x22), 0x0007000B: const Scancode(0x23),
  0x0007000C: const Scancode(0x17), 0x0007000D: const Scancode(0x24),
  0x0007000E: const Scancode(0x25), 0x0007000F: const Scancode(0x26),
  0x00070010: const Scancode(0x32), 0x00070011: const Scancode(0x31),
  0x00070012: const Scancode(0x18), 0x00070013: const Scancode(0x19),
  0x00070014: const Scancode(0x10), 0x00070015: const Scancode(0x13),
  0x00070016: const Scancode(0x1F), 0x00070017: const Scancode(0x14),
  0x00070018: const Scancode(0x16), 0x00070019: const Scancode(0x2F),
  0x0007001A: const Scancode(0x11), 0x0007001B: const Scancode(0x2D),
  0x0007001C: const Scancode(0x15), 0x0007001D: const Scancode(0x2C),
  // Digits 1–9, 0
  0x0007001E: const Scancode(0x02), 0x0007001F: const Scancode(0x03),
  0x00070020: const Scancode(0x04), 0x00070021: const Scancode(0x05),
  0x00070022: const Scancode(0x06), 0x00070023: const Scancode(0x07),
  0x00070024: const Scancode(0x08), 0x00070025: const Scancode(0x09),
  0x00070026: const Scancode(0x0A), 0x00070027: const Scancode(0x0B),
  // Enter, Escape, Backspace, Tab, Space
  0x00070028: const Scancode(0x1C), 0x00070029: const Scancode(0x01),
  0x0007002A: const Scancode(0x0E), 0x0007002B: const Scancode(0x0F),
  0x0007002C: const Scancode(0x39),
  // - = [ ] \ ; ' ` , . /
  0x0007002D: const Scancode(0x0C), 0x0007002E: const Scancode(0x0D),
  0x0007002F: const Scancode(0x1A), 0x00070030: const Scancode(0x1B),
  0x00070031: const Scancode(0x2B), 0x00070033: const Scancode(0x27),
  0x00070034: const Scancode(0x28), 0x00070035: const Scancode(0x29),
  0x00070036: const Scancode(0x33), 0x00070037: const Scancode(0x34),
  0x00070038: const Scancode(0x35),
  // Non-US \ (ISO key next to left Shift)
  0x00070064: const Scancode(0x56),
  // Caps Lock, F1–F12
  0x00070039: const Scancode(0x3A),
  0x0007003A: const Scancode(0x3B), 0x0007003B: const Scancode(0x3C),
  0x0007003C: const Scancode(0x3D), 0x0007003D: const Scancode(0x3E),
  0x0007003E: const Scancode(0x3F), 0x0007003F: const Scancode(0x40),
  0x00070040: const Scancode(0x41), 0x00070041: const Scancode(0x42),
  0x00070042: const Scancode(0x43), 0x00070043: const Scancode(0x44),
  0x00070044: const Scancode(0x57), 0x00070045: const Scancode(0x58),
  // Print Screen, Scroll Lock, Pause
  0x00070046: const Scancode(0x37, extended: _ext), 0x00070047: const Scancode(0x46),
  // Insert, Home, Page Up, Delete, End, Page Down
  0x00070049: const Scancode(0x52, extended: _ext), 0x0007004A: const Scancode(0x47, extended: _ext),
  0x0007004B: const Scancode(0x49, extended: _ext), 0x0007004C: const Scancode(0x53, extended: _ext),
  0x0007004D: const Scancode(0x4F, extended: _ext), 0x0007004E: const Scancode(0x51, extended: _ext),
  // Arrows: right, left, down, up
  0x0007004F: const Scancode(0x4D, extended: _ext), 0x00070050: const Scancode(0x4B, extended: _ext),
  0x00070051: const Scancode(0x50, extended: _ext), 0x00070052: const Scancode(0x48, extended: _ext),
  // Num Lock and keypad
  0x00070053: const Scancode(0x45),
  0x00070054: const Scancode(0x35, extended: _ext), 0x00070055: const Scancode(0x37),
  0x00070056: const Scancode(0x4A), 0x00070057: const Scancode(0x4E),
  0x00070058: const Scancode(0x1C, extended: _ext),
  0x00070059: const Scancode(0x4F), 0x0007005A: const Scancode(0x50),
  0x0007005B: const Scancode(0x51), 0x0007005C: const Scancode(0x4B),
  0x0007005D: const Scancode(0x4C), 0x0007005E: const Scancode(0x4D),
  0x0007005F: const Scancode(0x47), 0x00070060: const Scancode(0x48),
  0x00070061: const Scancode(0x49), 0x00070062: const Scancode(0x52),
  0x00070063: const Scancode(0x53),
  // Context menu
  0x00070065: const Scancode(0x5D, extended: _ext),
  // Modifiers: left Ctrl, Shift, Alt, Windows; right Ctrl, Shift, Alt, Windows
  0x000700E0: const Scancode(0x1D), 0x000700E1: const Scancode(0x2A),
  0x000700E2: const Scancode(0x38), 0x000700E3: const Scancode(0x5B, extended: _ext),
  0x000700E4: const Scancode(0x1D, extended: _ext), 0x000700E5: const Scancode(0x36),
  0x000700E6: const Scancode(0x38, extended: _ext), 0x000700E7: const Scancode(0x5C, extended: _ext),
};
