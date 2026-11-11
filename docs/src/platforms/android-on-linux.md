# Android Development on Linux

CraftUI provides a first-class developer experience for Android on Linux without requiring heavyweight IDEs or proprietary tooling.

---

## 1. Quick Device Verification

Connect your Android phone or start an emulator, and test ADB communication:

```bash
adb devices
```
If your device shows `device`, you are ready.

Run `craftui doctor` to verify your NDK and ADB paths:
```bash
craftui doctor
```

---

## 2. Manifest Configuration (`craftui.toml`)

CraftUI eliminates manual editing of complex Android XML manifests:

```toml
[app]
name = "CraftScan"
id = "dev.craftui.scancraft"
version = "0.1.0"

[android]
package = "dev.craftui.scancraft"
min_sdk = 26
target_sdk = 34
version_code = 100

[permissions]
camera = "Required to capture documents"
photos = "Required to select existing photos"
vibrate = "Provides haptic feedback on scan"
notifications = "Alerts when document processing is complete"
```

Running `craftui permissions check` validates that:
- Android `<uses-permission>` tags match official Google API standards.
- Obsolete permissions are automatically scoped with `maxSdkVersion` (e.g. `READ_EXTERNAL_STORAGE` on SDK <= 32).
- Feature flags like `<uses-feature android:name="android.hardware.camera" android:required="false" />` are generated so your app can install on devices without cameras.

---

## 3. One-Command Build and Run

To compile, build the APK, install to your phone, and launch the activity:

```bash
craftui run --platform android
```

CraftUI performs the following automatically:
1. Compiles the Rust workspace using your local NDK `clang` toolchain (`aarch64-linux-android`).
2. Bundles the compiled native `.so` library into the Android `jniLibs/arm64-v8a` directory.
3. Invokes Gradle to package the debug APK.
4. Executes `adb install -r` and starts the `GameActivity` intent.
5. Streams real-time `logcat` output directly to your terminal.

---

## 4. Hardware Acceleration & Insets
- **GPU Rendering:** CraftUI renders directly to the native `ANativeWindow` surface using Vulkan or OpenGL ES via WGPU.
- **Edge-to-Edge & Software Keyboard:** Real-time WindowInsets updates automatically populate the `SafeArea` widget, so on-screen keyboards smoothly slide the UI upward.
