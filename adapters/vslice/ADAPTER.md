# V-Slice adapter — planned

Initial target: official public V-Slice source as an independently built runtime.

Required work:
1. Pin a specific upstream commit/tag.
2. Reproduce a clean build for each target platform.
3. Record Lime/OpenFL/Haxe/HaxeFlixel and native extension versions.
4. Define launch arguments, working directory, asset root, save root and exit behavior.
5. Add smoke tests before marking `integrated`.

Do not assume the Play Store APK's version maps exactly to a public source tag.
