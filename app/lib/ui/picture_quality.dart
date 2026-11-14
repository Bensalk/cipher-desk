import 'package:flutter/material.dart';

import '../src/rust/api/session.dart' show PictureQuality;

/// Names and help for the picture quality setting.
extension PictureQualityText on PictureQuality {
  String get label => switch (this) {
        PictureQuality.best => 'Best quality',
        PictureQuality.balanced => 'Balanced',
        PictureQuality.speed => 'Speed',
      };

  String get description => switch (this) {
        PictureQuality.best =>
          'Exact pixels, sharpest text (with Share screen). Uses the most data: best on a fast connection.',
        PictureQuality.balanced => 'Sharp picture with compression. Good for most connections.',
        PictureQuality.speed => 'Least data and quickest response on a slow connection. Softer picture.',
      };

  IconData get icon => switch (this) {
        PictureQuality.best => Icons.high_quality_outlined,
        PictureQuality.balanced => Icons.tune,
        PictureQuality.speed => Icons.speed,
      };
}
