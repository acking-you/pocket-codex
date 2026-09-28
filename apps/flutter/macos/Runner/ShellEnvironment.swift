import Foundation

enum ShellEnvironment {
  // Run before Flutter starts worker threads or restores the local host.
  static func restore() {
    if let path = resolvedPath(environment: ProcessInfo.processInfo.environment) {
      setenv("PATH", path, 1)
    }
  }

  static func resolvedPath(
    environment: [String: String], timeout: TimeInterval = 3
  ) -> String? {
    let fm = FileManager.default
    let directory = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    do {
      try fm.createDirectory(
        at: directory, withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700]
      )
    } catch {
      return nil
    }
    defer { try? fm.removeItem(at: directory) }

    let output = directory.appendingPathComponent("path")
    let process = Process()
    let accountShell = getpwuid(getuid()).flatMap { entry in
      entry.pointee.pw_shell.map { String(cString: $0) }
    }
    let shell = environment["SHELL"] ?? accountShell ?? "/bin/zsh"
    guard shell.hasPrefix("/"), fm.isExecutableFile(atPath: shell) else { return nil }
    process.executableURL = URL(fileURLWithPath: shell)
    // Capture PATH separately: shell startup files may print banners/warnings.
    // printenv also handles fish's list-valued PATH without shell-specific syntax.
    process.arguments = ["-ilc", "/usr/bin/printenv PATH > \"$POCKET_CODEX_PATH_FILE\""]
    var childEnvironment = environment
    childEnvironment["POCKET_CODEX_PATH_FILE"] = output.path
    process.environment = childEnvironment
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice

    let finished = DispatchSemaphore(value: 0)
    process.terminationHandler = { _ in finished.signal() }
    do {
      try process.run()
    } catch {
      return nil
    }
    guard finished.wait(timeout: .now() + timeout) == .success else {
      // SIGKILL is bounded even when a startup file traps/ignores SIGTERM.
      if process.isRunning { kill(process.processIdentifier, SIGKILL) }
      process.waitUntilExit()
      return nil
    }
    guard process.terminationStatus == 0,
      let file = try? FileHandle(forReadingFrom: output)
    else { return nil }
    defer { try? file.close() }
    let data = file.readData(ofLength: 128 * 1024)
    guard data.count < 128 * 1024,
      var shellPath = String(data: data, encoding: .utf8)
    else { return nil }
    if shellPath.hasSuffix("\n") { shellPath.removeLast() }
    guard !shellPath.isEmpty, !shellPath.utf8.contains(0) else { return nil }

    var seen = Set<String>()
    let entries = (shellPath + ":" + (environment["PATH"] ?? ""))
      .components(separatedBy: ":")
      .filter { !$0.isEmpty && seen.insert($0).inserted }
    return entries.isEmpty ? nil : entries.joined(separator: ":")
  }
}
