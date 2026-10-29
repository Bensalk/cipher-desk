import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:secure_rdp_viewer/session/scancodes.dart';

void main() {
  test('common keys map to PC scancodes', () {
    expect(scancodeFor(PhysicalKeyboardKey.keyA)?.code, 0x1E);
    expect(scancodeFor(PhysicalKeyboardKey.enter)?.code, 0x1C);
    expect(scancodeFor(PhysicalKeyboardKey.escape)?.code, 0x01);
    expect(scancodeFor(PhysicalKeyboardKey.f12)?.code, 0x58);
  });

  test('extended keys are flagged', () {
    final up = scancodeFor(PhysicalKeyboardKey.arrowUp);
    expect(up?.code, 0x48);
    expect(up?.extended, isTrue);
    expect(scancodeFor(PhysicalKeyboardKey.controlRight)?.extended, isTrue);
    expect(scancodeFor(PhysicalKeyboardKey.controlLeft)?.extended, isFalse);
  });

  test('unmapped keys return null', () {
    expect(scancodeFor(PhysicalKeyboardKey.fn), isNull);
  });
}
