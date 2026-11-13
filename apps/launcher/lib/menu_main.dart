import 'package:flutter/material.dart';

import 'ui/fnf_app.dart';

/// Phase 1 entry point for the new main-menu UI.
///
/// Run with: flutter run -t lib/menu_main.dart
///
/// lib/main.dart is intentionally untouched so the existing app and its tests
/// keep working. Once this entry point is verified on device, switching the
/// default is a one-line change in main.dart (runApp(const FnfMenuApp())).
void main() => runApp(const FnfMenuApp());
