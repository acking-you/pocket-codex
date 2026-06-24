import Cocoa
import FlutterMacOS

@main
class AppDelegate: FlutterAppDelegate {
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return true
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  // Single instance: macOS reuses the running .app when it's launched again
  // (LaunchServices), delivering a reopen here instead of a second process. If
  // the window was hidden to the tray, surface it.
  override func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    if !flag {
      // Only restore real top-level windows; canBecomeKey filters out hidden
      // helper/background windows some plugins create.
      for window in sender.windows where window.canBecomeKey {
        window.makeKeyAndOrderFront(self)
      }
      NSApp.activate(ignoringOtherApps: true)
    }
    return true
  }
}
