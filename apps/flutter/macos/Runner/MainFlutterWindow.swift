import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  /// Height of the app's own top strip (`WindowChrome.barHeight` in Dart).
  /// The traffic lights are centred in it, the way a unified toolbar window
  /// places them, so the sidebar and header controls share their centre line.
  static let titleStripHeight: CGFloat = 52

  /// Leading inset of the close button, matching AppKit's toolbar layout.
  static let trafficLightInset: CGFloat = 20

  private var trafficLightObservers: [NSObjectProtocol] = []

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    // Theme-aware Dock icon: macOS ships one AppIcon per bundle, so matching it
    // to the app's light/dark appearance means assigning it at runtime.
    DockIcon.register(
      with: flutterViewController.registrar(forPlugin: "DockIcon")
    )
    // Dart hides the title bar (window_manager) after this runs, and then
    // asks for the buttons to be laid out against it.
    let chrome = FlutterMethodChannel(
      name: "pocket_codex/window_chrome",
      binaryMessenger: flutterViewController.engine.binaryMessenger
    )
    chrome.setMethodCallHandler { [weak self] call, result in
      guard call.method == "layoutTrafficLights" else {
        result(FlutterMethodNotImplemented)
        return
      }
      self?.layoutTrafficLights()
      result(nil)
    }

    super.awakeFromNib()
    observeTrafficLights()
  }

  deinit {
    trafficLightObservers.forEach(NotificationCenter.default.removeObserver)
  }

  /// AppKit re-lays out the title bar container on resize, fullscreen changes
  /// and key-state changes, which snaps the buttons back to their default
  /// 28 pt position. Re-centre them after each of those.
  private func observeTrafficLights() {
    let names: [Notification.Name] = [
      NSWindow.didResizeNotification,
      NSWindow.didEndLiveResizeNotification,
      NSWindow.didBecomeKeyNotification,
      NSWindow.didResignKeyNotification,
      NSWindow.didExitFullScreenNotification,
      NSWindow.didChangeBackingPropertiesNotification,
    ]
    for name in names {
      let token = NotificationCenter.default.addObserver(
        forName: name, object: self, queue: .main
      ) { [weak self] _ in
        self?.layoutTrafficLights()
      }
      trafficLightObservers.append(token)
    }
    DispatchQueue.main.async { [weak self] in self?.layoutTrafficLights() }
  }

  /// Centre the three standard buttons vertically in [titleStripHeight] and
  /// inset them from the leading edge. Only while the title bar is
  /// transparent (the Dart side hides it at startup); in fullscreen AppKit
  /// owns the buttons and they are left alone.
  func layoutTrafficLights() {
    guard titlebarAppearsTransparent,
      !styleMask.contains(.fullScreen),
      let close = standardWindowButton(.closeButton),
      let mini = standardWindowButton(.miniaturizeButton),
      let zoom = standardWindowButton(.zoomButton),
      let container = close.superview?.superview
    else { return }

    let strip = Self.titleStripHeight
    let buttonHeight = close.frame.height
    // Grow the title bar container to the strip so the buttons stay inside
    // it (and keep their hover/click tracking) once moved down.
    var frame = container.frame
    frame.size.height = strip
    frame.origin.y = self.frame.height - strip
    container.frame = frame

    let spacing = mini.frame.minX - close.frame.minX
    let y = (strip - buttonHeight) / 2
    for (index, button) in [close, mini, zoom].enumerated() {
      button.setFrameOrigin(
        NSPoint(x: Self.trafficLightInset + CGFloat(index) * spacing, y: y)
      )
    }
  }
}
