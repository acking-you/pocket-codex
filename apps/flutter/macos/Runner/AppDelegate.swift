import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  /// Closing hides to the tray, so the app has to outlive its window.
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return false
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  /// Bring the main window back when the app is reopened (Dock icon click,
  /// second launch — LaunchServices delivers both here).
  ///
  /// Deliberately ignores `hasVisibleWindows`: plugins (webview, drag & drop,
  /// the status item) own helper NSWindows that can count as "visible" while
  /// the real window is hidden to the tray, which would make a flag-guarded
  /// restore silently do nothing — the window could then never be reopened
  /// from the Dock. Restoring the main window unconditionally is also what a
  /// Dock click should do anyway. Targets MainFlutterWindow explicitly (never
  /// the helpers) and uses setIsVisible(true) first: makeKeyAndOrderFront
  /// alone does not reliably restore a window hidden via setIsVisible(false).
  override func applicationShouldHandleReopen(
    _ sender: NSApplication,
    hasVisibleWindows flag: Bool
  ) -> Bool {
    let main =
      sender.windows.first { $0 is MainFlutterWindow }
      ?? sender.windows.first { $0.contentViewController is FlutterViewController }
    if let window = main {
      if window.isMiniaturized {
        window.deminiaturize(self)
      }
      window.setIsVisible(true)
      window.makeKeyAndOrderFront(self)
    }
    sender.activate(ignoringOtherApps: true)
    return false
  }
}
