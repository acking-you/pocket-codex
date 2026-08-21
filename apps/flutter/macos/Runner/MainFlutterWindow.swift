import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
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

    super.awakeFromNib()
  }
}
