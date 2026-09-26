// Run without Flutter/Xcode: swiftc Runner/ShellEnvironment.swift
// RunnerTests/ShellEnvironmentCheck.swift -o /tmp/shell-environment-check
import Foundation

@main
enum ShellEnvironmentCheck {
  static func main() throws {
    let fm = FileManager.default
    let root = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try fm.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? fm.removeItem(at: root) }

    func script(_ name: String, _ text: String) throws -> URL {
      let url = root.appendingPathComponent(name)
      try Data(text.utf8).write(to: url)
      try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
      return url
    }
    func check(_ condition: Bool, _ message: String) {
      guard condition else {
        fputs("FAIL: \(message)\n", stderr)
        exit(1)
      }
      print("PASS: \(message)")
    }

    let nodeBin = root.appendingPathComponent("node with spaces")
    try fm.createDirectory(at: nodeBin, withIntermediateDirectories: true)
    let node = nodeBin.appendingPathComponent("node")
    try Data("#!/bin/sh\nprintf 'node-ok'\n".utf8).write(to: node)
    try fm.setAttributes([.posixPermissions: 0o700], ofItemAtPath: node.path)
    let codex = try script("codex", "#!/usr/bin/env node\n")
    let shell = try script("login-shell", """
      #!/bin/sh
      [ "$1" = '-ilc' ] || exit 9
      printf 'startup noise\\n'
      printf 'startup warning\\n' >&2
      export PATH="$TEST_NODE_BIN:/usr/bin:/bin"
      eval "$2"
      """)
    let inherited = "/usr/bin:/bin:/custom/bin"
    let environment = [
      "PATH": inherited, "SHELL": shell.path, "TEST_NODE_BIN": nodeBin.path,
    ]

    func runCodex(path: String) throws -> (Int32, String) {
      let process = Process()
      process.executableURL = codex
      process.environment = ["PATH": path]
      let output = Pipe()
      process.standardOutput = output
      process.standardError = output
      try process.run()
      let data = output.fileHandleForReading.readDataToEndOfFile()
      process.waitUntilExit()
      return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }
    let broken = try runCodex(path: inherited)
    check(broken.0 == 127 && broken.1.contains("node"), "sparse PATH reproduces missing node")
    let resolved = ShellEnvironment.resolvedPath(environment: environment)
    let repaired = try runCodex(path: resolved ?? inherited)
    check(repaired.0 == 0 && repaired.1 == "node-ok", "npm-style Codex starts with resolved PATH")
    let entries = (resolved ?? "").components(separatedBy: ":")
    check(entries.first == nodeBin.path, "shell-selected Node has precedence; spaces survive")
    check(entries.contains("/custom/bin"), "inherited PATH entries survive")
    check(entries.filter { $0 == "/usr/bin" }.count == 1, "PATH entries are deduplicated")
    check(!(resolved ?? "").contains("startup"), "shell startup output cannot pollute PATH")

    let failing = try script("failing-shell", "#!/bin/sh\nexit 2\n")
    check(ShellEnvironment.resolvedPath(environment: ["SHELL": failing.path]) == nil,
          "failed shell leaves inherited PATH alone")
    let hanging = try script("hanging-shell", "#!/bin/sh\ntrap '' TERM\nwhile :; do :; done\n")
    let start = ProcessInfo.processInfo.systemUptime
    check(ShellEnvironment.resolvedPath(environment: ["SHELL": hanging.path], timeout: 0.1) == nil,
          "hung shell falls back")
    check(ProcessInfo.processInfo.systemUptime - start < 1.5,
          "shell ignoring SIGTERM cannot block startup")

    let empty = try script("empty-shell", "#!/bin/sh\nprintf '\\n' > \"$POCKET_CODEX_PATH_FILE\"\n")
    check(ShellEnvironment.resolvedPath(environment: ["SHELL": empty.path]) == nil,
          "empty PATH leaves inherited PATH alone")
  }
}
