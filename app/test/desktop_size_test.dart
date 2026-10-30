import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:cipher_desk/ui/session_screen.dart';

void main() {
  test('computers ask for the window size, within limits', () {
    expect(desktopSizeFor(const Size(1280, 720), phoneOrTablet: false), (1280, 720));
    expect(desktopSizeFor(const Size(1281, 721), phoneOrTablet: false), (1280, 720));
    expect(desktopSizeFor(const Size(300, 200), phoneOrTablet: false), (800, 600));
    expect(desktopSizeFor(const Size(7680, 4320), phoneOrTablet: false), (3840, 2160));
  });

  test('phones and tablets always ask for a landscape desktop up to 1920x1080', () {
    // Portrait phone 1080x2400 → landscape, capped.
    expect(desktopSizeFor(const Size(1080, 2400), phoneOrTablet: true), (1920, 1080));
    // Landscape tablet 1600x1000 fits as it is.
    expect(desktopSizeFor(const Size(1600, 1000), phoneOrTablet: true), (1600, 1000));
    // Portrait tablet 800x1280 → 1280x800.
    expect(desktopSizeFor(const Size(800, 1280), phoneOrTablet: true), (1280, 800));
  });
}
