import 'package:flutter/foundation.dart';

/// Session-level Developer Mode switch.
///
/// UI state only; it never touches engine state or project data. The spec says
/// the unlock lives in a hidden control inside Controller Editor. That editor
/// does not exist yet, so the menu currently calls [unlock] from a temporary
/// hidden gesture. When Controller Editor lands, call [unlock] from there and
/// remove the temporary gesture.
class DeveloperModeController extends ChangeNotifier {
  bool _unlocked = false;

  bool get unlocked => _unlocked;

  void unlock() {
    if (_unlocked) return;
    _unlocked = true;
    notifyListeners();
  }

  void lock() {
    if (!_unlocked) return;
    _unlocked = false;
    notifyListeners();
  }
}
