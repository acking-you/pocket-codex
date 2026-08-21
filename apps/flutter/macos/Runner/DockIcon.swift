import Cocoa
import FlutterMacOS

/// Swaps the running app's Dock icon to match the app's appearance.
///
/// macOS has no asset-catalog equivalent of iOS's light/dark app-icon variants:
/// the bundle ships ONE `AppIcon`, and the only way to show a different one is
/// to assign `NSApp.applicationIconImage` at runtime. That is process-scoped and
/// deliberately not persisted — quitting restores the bundle's icon, which is
/// what we want, since the choice lives in the app's own prefs and is re-applied
/// on the next launch.
///
/// Dart owns the decision (it knows whether the user picked light/dark or is
/// following the system) and calls `setAppearance`; this side only resolves the
/// matching image out of the Flutter asset bundle. Passing `nil`/"system"
/// restores the bundle icon so the OS renders whatever it normally would.
enum DockIcon {
  private static let channelName = "pocket_codex/dock_icon"

  static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(
      name: channelName,
      binaryMessenger: registrar.messenger
    )
    channel.setMethodCallHandler { call, result in
      guard call.method == "setAppearance" else {
        result(FlutterMethodNotImplemented)
        return
      }
      let mode = (call.arguments as? [String: Any])?["mode"] as? String
      apply(mode: mode, registrar: registrar)
      result(nil)
    }
  }

  /// `mode` is "light" / "dark"; anything else (including nil) hands the icon
  /// back to the bundle.
  private static func apply(mode: String?, registrar: FlutterPluginRegistrar) {
    guard let mode, mode == "light" || mode == "dark" else {
      NSApp.applicationIconImage = nil
      return
    }
    // The tile masters ride along as Flutter assets. On macOS `lookupKey`
    // returns a path relative to the APP BUNDLE ROOT (it already includes
    // `Contents/Frameworks/App.framework/...`), so it must be joined onto the
    // bundle URL directly — handing it to `Bundle.url(forResource:)`, which
    // searches a bundle's Resources, finds nothing and silently leaves the icon
    // alone.
    let key = registrar.lookupKey(forAsset: "assets/logo/mark_\(mode).png")
    let url = Bundle.main.bundleURL.appendingPathComponent(key)
    guard let image = NSImage(contentsOf: url) else {
      // A missing asset must not blank the Dock icon: leave whatever is there.
      return
    }
    NSApp.applicationIconImage = image
  }
}
