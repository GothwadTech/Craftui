# Installation & Environment Setup (Linux & Android)

CraftUI is designed to be completely self-sufficient on **Linux**. You can build, run, and test your applications for **Linux Desktop**, **Android** (physical phones, tablets, or emulators), and the **Web** using only standard Linux developer tools.

---

## 1. Prerequisites on Linux

Ensure you have a recent version of Rust (Edition 2024 / Rust 1.89+) installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

Install standard development libraries for WGPU and Wayland/X11 windowing:

```bash
# Ubuntu / Debian / Pop!_OS
sudo apt update
sudo apt install -y build-essential pkg-config libx11-dev libxcursor-dev \
    libxrandr-dev libxi-dev libxinerama-dev libwayland-dev libxkbcommon-dev \
    libvulkan-dev vulkan-tools libasound2-dev libfontconfig1-dev
```

---

## 2. Setting Up the Android Toolchain on Linux

You do **not** need Android Studio to build CraftUI Android apps. A minimal Android command-line SDK + NDK is sufficient:

1. Add the `aarch64-linux-android` Rust compilation target:
   ```bash
   rustup target add aarch64-linux-android
   ```

2. Point your environment variables to your Android SDK and NDK (e.g., in `~/.bashrc`):
   ```bash
   export ANDROID_HOME="$HOME/Android/Sdk"
   export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/28.2.13676358"
   export PATH="$ANDROID_HOME/platform-tools:$PATH"
   ```

3. Verify your device is connected via ADB:
   ```bash
   adb devices
   ```

---

## 3. Installing the CraftUI Tooling

Build and install `craftui-cli`:

```bash
cargo install --path tools/craftui-cli
```

Verify your environment with the built-in diagnostic tool:

```bash
craftui doctor
```

Output:
```
CraftUI Environment Doctor
--------------------------
[ok] rustc: rustc 1.89+
[ok] cargo: cargo 1.89+
[ok] Desktop target (host): ready
[ok] Web target (wasm32-unknown-unknown): installed
[ok] Android target (aarch64-linux-android): installed (NDK: /home/.../ndk/28.2.13676358, adb: /home/.../platform-tools/adb)
[ok] Templates (.cui): verified
```

You are now ready to build native applications for Linux and Android!
