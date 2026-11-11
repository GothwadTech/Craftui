# Mobile & Web Permissions Guide

CraftUI features a unified permission system spanning 35 standard permissions across Android, iOS, and Web.

---

## 1. The 35 Standard Permissions

Inspect all available permissions at any time with the CLI:

```bash
craftui permissions list
```

| Permission Key | Description | Android Mapping | iOS Usage Description |
|---|---|---|---|
| `camera` | Camera capture | `android.permission.CAMERA` | `NSCameraUsageDescription` |
| `microphone` | Audio recording | `android.permission.RECORD_AUDIO` | `NSMicrophoneUsageDescription` |
| `location_when_in_use` | Foreground GPS | `ACCESS_FINE_LOCATION`, `ACCESS_COARSE_LOCATION` | `NSLocationWhenInUseUsageDescription` |
| `photos` | Photo library read | `READ_MEDIA_IMAGES` (API 33+), `READ_EXTERNAL_STORAGE` (API <= 32) | `NSPhotoLibraryUsageDescription` |
| `biometrics` | Fingerprint / Face | `USE_BIOMETRIC` | `NSFaceIDUsageDescription` |
| `contacts` | Address book | `READ_CONTACTS`, `WRITE_CONTACTS` | `NSContactsUsageDescription` |
| `bluetooth` | BLE & Classic | `BLUETOOTH_SCAN`, `BLUETOOTH_CONNECT`, `BLUETOOTH_ADVERTISE` (API 31+) | `NSBluetoothAlwaysUsageDescription` |
| `vibrate` | Haptic feedback | `android.permission.VIBRATE` | *(Built-in CoreHaptics)* |
| `notifications` | Push alerts | `POST_NOTIFICATIONS` (API 33+) | *(UserNotifications Framework)* |

---

## 2. Manifest Reference (`craftui.toml`)

Enable permissions with human-readable rationale strings in your `craftui.toml`:

```toml
[app]
name = "MyPhotoApp"
id = "dev.craftui.photo"
version = "1.0.0"

[permissions]
camera = "Allows you to take pictures directly in the app."
photos = "Allows you to choose photos from your gallery."
vibrate = "Provides haptic feedback when taking pictures."
```

Audit your manifest against OS store policies:
```bash
craftui permissions check
```

---

## 3. Requesting Permissions in Rust Code

Use the `PlatformServices` API to check and request permissions at runtime:

```rust
use craftui::prelude::*;

async fn take_photo(services: &PlatformServices) {
    // 1. Check current status
    let status = services.permission_status(Permission::Camera).await;

    match status {
        PermissionStatus::Granted => {
            // Permission already granted, proceed
            capture_image().await;
        }
        PermissionStatus::NotDetermined | PermissionStatus::Denied => {
            // 2. Request permission from user
            let new_status = services.request_permission(Permission::Camera).await;
            if new_status == PermissionStatus::Granted {
                capture_image().await;
            } else {
                println!("Camera access was denied by user.");
            }
        }
        PermissionStatus::Restricted => {
            println!("Camera access is restricted by device policy.");
        }
    }
}
```
On Android, this invokes the native `ActivityResultContracts.RequestMultiplePermissions` dialog. On iOS, it prompts the system privacy dialog. On Desktop, it resolves immediately to granted or simulated status.
