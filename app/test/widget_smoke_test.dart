import 'package:flutter_test/flutter_test.dart';

/// Pure-Dart test placeholder so `flutter test` has a target without loading
/// the native library. Rust-backed behaviour is covered by integration_test/.
void main() {
  test('app name is set', () {
    const name = 'Secure RDP Viewer';
    expect(name.isNotEmpty, isTrue);
  });
}
