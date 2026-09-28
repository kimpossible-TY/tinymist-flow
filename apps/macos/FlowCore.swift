import Foundation
import Darwin

let bundleID = "io.github.kimpossible-ty.tinymist-flow"
let appName = "tinymist-flow"
let fm = FileManager.default
let userHome = fm.homeDirectoryForCurrentUser
let support = ProcessInfo.processInfo.environment["FLOW_DATA_DIR"].map { URL(fileURLWithPath: $0) }
    ?? userHome.appendingPathComponent("Library/Application Support/tinymist-flow")
let logRoot = ProcessInfo.processInfo.environment["FLOW_LOG_DIR"].map { URL(fileURLWithPath: $0) }
    ?? userHome.appendingPathComponent("Library/Logs/tinymist-flow")
let agents = userHome.appendingPathComponent("Library/LaunchAgents")
let executable = Bundle.main.executableURL!
let engine = executable.deletingLastPathComponent().appendingPathComponent("flow-engine")

struct FlowError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw FlowError(message: message) }
}
func matches(_ value: String, _ pattern: String) -> Bool {
    value.range(of: pattern, options: .regularExpression) != nil
}
struct Profile: Codable {
    var id: String
    var name: String
    var root: String
    var entry: String
    var fonts: [String]
    var packages: String
    var port: Int
    var publicURL: String
    var label: String
    var focusFile: String { support.appendingPathComponent("focus/\(id).json").path }
    var url: String { publicURL.isEmpty ? "http://127.0.0.1:\(port)/" : publicURL }
    func path(_ relative: String) -> String {
        (relative.hasPrefix("/") ? URL(fileURLWithPath: relative) : URL(fileURLWithPath: root).appendingPathComponent(relative)).standardizedFileURL.path
    }
    func validate(checkFiles: Bool = false) throws {
        try require(matches(id, "^[a-z0-9][a-z0-9-]{0,63}$"), "프로젝트 ID가 올바르지 않습니다.")
        try require(!name.trimmingCharacters(in: .whitespaces).isEmpty, "프로젝트 이름을 입력하세요.")
        try require(root.hasPrefix("/") && !entry.isEmpty, "프로젝트 폴더와 진입 파일을 지정하세요.")
        let rootURL = URL(fileURLWithPath: root).standardizedFileURL.path
        let entryPath = path(entry)
        try require(entryPath.hasPrefix(rootURL + "/") && entryPath.hasSuffix(".typ"), "진입 파일은 프로젝트 폴더 안의 .typ 파일이어야 합니다.")
        try require((1024...65535).contains(port), "포트는 1024–65535 범위여야 합니다.")
        try require(matches(label, "^[a-zA-Z0-9][a-zA-Z0-9.-]+$"), "LaunchAgent 이름이 올바르지 않습니다.")
        if !publicURL.isEmpty {
            guard let u = URLComponents(string: publicURL) else { throw FlowError(message: "HTTPS 주소가 올바르지 않습니다.") }
            try require(u.scheme == "https" && u.host != nil && u.user == nil && u.password == nil &&
                        u.query == nil && u.fragment == nil && (u.path == "" || u.path == "/"), "공개 주소에는 HTTPS 호스트와 포트만 입력하세요.")
        }
        if checkFiles {
            try require(fm.fileExists(atPath: entryPath), "진입 파일을 읽을 수 없습니다: \(entryPath)")
            for font in fonts { try require(fm.fileExists(atPath: path(font)), "글꼴 폴더를 찾을 수 없습니다: \(path(font))") }
            if !packages.isEmpty { try require(fm.fileExists(atPath: packages), "패키지 폴더를 찾을 수 없습니다.") }
        }
    }
    func arguments() -> [String] {
        var args = ["preview", path(entry), "--root", root, "--data-plane-host=127.0.0.1:\(port)",
                    "--control-plane-host=127.0.0.1:0", "--no-open", "--partial-rendering=true", "--preview-mode=document"]
        for font in fonts { args += ["--font-path", path(font)] }
        if !packages.isEmpty { args += ["--package-path", packages] }
        return args
    }
    func environment() -> [String: String] {
        var env = ["TINYMIST_PREVIEW_FOCUS_FILE": focusFile]
        if !publicURL.isEmpty { env["TINYMIST_ALLOWED_ORIGINS"] = String(publicURL.trimmingCharacters(in: CharacterSet(charactersIn: "/"))) }
        return env
    }
}
struct Settings: Codable {
    var schemaVersion = 1
    var selectedID: String = ""
    var profiles: [Profile] = []
    func validate() throws {
        try require(schemaVersion == 1, "지원하지 않는 설정 버전입니다.")
        try require(Set(profiles.map(\.id)).count == profiles.count, "프로젝트 ID가 중복됩니다.")
        try require(Set(profiles.map(\.port)).count == profiles.count, "프로젝트 포트가 중복됩니다.")
        try require(Set(profiles.map(\.label)).count == profiles.count, "LaunchAgent 이름이 중복됩니다.")
        for p in profiles { try p.validate() }
    }
    func profile(_ id: String) throws -> Profile {
        guard let p = profiles.first(where: { $0.id == id }) else { throw FlowError(message: "프로젝트를 찾을 수 없습니다: \(id)") }
        return p
    }
}
func loadSettings() throws -> Settings {
    let file = support.appendingPathComponent("profiles.json")
    guard fm.fileExists(atPath: file.path) else { return Settings() }
    let s = try JSONDecoder().decode(Settings.self, from: Data(contentsOf: file))
    try s.validate()
    return s
}
func saveSettings(_ settings: Settings) throws {
    try settings.validate()
    try fm.createDirectory(at: support, withIntermediateDirectories: true)
    let enc = JSONEncoder(); enc.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
    let file = support.appendingPathComponent("profiles.json")
    try enc.encode(settings).write(to: file, options: .atomic)
    try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
}
@discardableResult
func command(_ path: String, _ args: [String], allowFailure: Bool = false) throws -> String {
    let task = Process(); task.executableURL = URL(fileURLWithPath: path); task.arguments = args
    let pipe = Pipe(); task.standardOutput = pipe; task.standardError = pipe
    try task.run()
    let data = pipe.fileHandleForReading.readDataToEndOfFile(); task.waitUntilExit()
    let output = String(data: data, encoding: .utf8) ?? ""
    try require(allowFailure || task.terminationStatus == 0, "\(path): \(output.trimmingCharacters(in: .whitespacesAndNewlines))")
    return output
}
func jobPID(_ label: String) -> Int? {
    let result = try? command("/bin/launchctl", ["print", "gui/\(getuid())/\(label)"])
    guard let result, let range = result.range(of: "(?m)^\\s*pid = [0-9]+$", options: .regularExpression) else { return nil }
    return Int(result[range].split(separator: "=").last!.trimmingCharacters(in: .whitespaces))
}
func agent(_ p: Profile) -> [String: Any] {
    ["Label": p.label, "ProgramArguments": [executable.path, "--serve", p.id],
     "AssociatedBundleIdentifiers": [bundleID], "RunAtLoad": true, "KeepAlive": true, "ThrottleInterval": 10,
     "WorkingDirectory": support.path, "EnvironmentVariables": p.environment(),
     "StandardOutPath": logRoot.appendingPathComponent("\(p.id).out.log").path,
     "StandardErrorPath": logRoot.appendingPathComponent("\(p.id).err.log").path]
}
func withControlLock<T>(_ body: () throws -> T) throws -> T {
    try fm.createDirectory(at: support, withIntermediateDirectories: true)
    let fd = open(support.appendingPathComponent("control.lock").path, O_CREAT | O_RDWR, 0o600)
    try require(fd >= 0, "제어 잠금을 열 수 없습니다.")
    defer { flock(fd, LOCK_UN); close(fd) }
    try require(flock(fd, LOCK_EX | LOCK_NB) == 0, "다른 서비스 작업이 진행 중입니다.")
    return try body()
}
func control(_ action: String, _ p: Profile) throws {
    try withControlLock {
        let target = "gui/\(getuid())/\(p.label)"
        if action == "stop" {
            try command("/bin/launchctl", ["disable", target])
            try command("/bin/launchctl", ["bootout", target], allowFailure: true)
            return
        }
        try require(action == "start" || action == "restart", "지원하지 않는 작업입니다.")
        try p.validate(checkFiles: true)
        let owners = (try? command("/usr/sbin/lsof", ["-nP", "-iTCP:\(p.port)", "-sTCP:LISTEN", "-t"], allowFailure: true)) ?? ""
        let ownerIDs = Set(owners.split(separator: "\n").compactMap { Int($0) })
        let pid = jobPID(p.label)
        try require(ownerIDs.isEmpty || ownerIDs == Set([pid].compactMap { $0 }), "포트 \(p.port)를 다른 프로세스가 사용 중입니다.")
        if action == "start" && pid != nil { return }
        try fm.createDirectory(at: logRoot, withIntermediateDirectories: true)
        try fm.createDirectory(at: support.appendingPathComponent("focus"), withIntermediateDirectories: true)
        try fm.createDirectory(at: agents, withIntermediateDirectories: true)
        let plist = agents.appendingPathComponent("\(p.label).plist")
        let data = try PropertyListSerialization.data(fromPropertyList: agent(p), format: .xml, options: 0)
        try data.write(to: plist, options: .atomic)
        try command("/bin/launchctl", ["bootout", target], allowFailure: true)
        try command("/bin/launchctl", ["enable", target])
        var last: Error?
        for _ in 0..<5 {
            do { try command("/bin/launchctl", ["bootstrap", "gui/\(getuid())", plist.path]); return }
            catch { last = error; Thread.sleep(forTimeInterval: 0.3) }
        }
        throw last!
    }
}
func serve(_ p: Profile) throws -> Never {
    try p.validate(checkFiles: true)
    try require(fm.isExecutableFile(atPath: engine.path), "앱에 포함된 엔진이 없습니다.")
    try fm.createDirectory(at: support.appendingPathComponent("focus"), withIntermediateDirectories: true)
    unsetenv("TINYMIST_ALLOWED_ORIGINS"); unsetenv("TINYMIST_PREVIEW_FOCUS_FILE")
    for (key, value) in p.environment() { setenv(key, value, 1) }
    let args = ([engine.path] + p.arguments()).map { strdup($0) } + [nil]
    args.withUnsafeBufferPointer { _ = execv(engine.path, $0.baseAddress!) }
    throw FlowError(message: "엔진 실행 실패: \(String(cString: strerror(errno)))")
}
func loginEnabled() -> Bool { fm.fileExists(atPath: agents.appendingPathComponent("\(bundleID).login.plist").path) }
func setLogin(_ enabled: Bool) throws {
    let label = "\(bundleID).login"
    let file = agents.appendingPathComponent("\(label).plist")
    if enabled {
        try fm.createDirectory(at: agents, withIntermediateDirectories: true)
        let job: [String: Any] = ["Label": label, "ProgramArguments": [executable.path, "--background"], "RunAtLoad": true,
                                  "AssociatedBundleIdentifiers": [bundleID]]
        try PropertyListSerialization.data(fromPropertyList: job, format: .xml, options: 0).write(to: file, options: .atomic)
        try command("/bin/launchctl", ["enable", "gui/\(getuid())/\(label)"])
    } else {
        try command("/bin/launchctl", ["disable", "gui/\(getuid())/\(label)"])
        if fm.fileExists(atPath: file.path) { try fm.removeItem(at: file) }
    }
}
func jsonOut(_ object: Any) throws {
    let data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys, .fragmentsAllowed])
    print(String(data: data, encoding: .utf8)!)
}
func runCLI(_ args: [String]) throws {
    if args.first == "--version" {
        print("tinymist-flow \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") ?? "dev")")
        return
    }
    let settings = try loadSettings()
    switch args.first {
    case "--check-config": try settings.validate(); print("ok")
    case "--profiles":
        let data = try JSONEncoder().encode(settings); print(String(data: data, encoding: .utf8)!)
    case "--control":
        try require(args.count == 3, "--control start|stop|restart PROJECT_ID")
        try control(args[1], settings.profile(args[2]))
    case "--serve":
        try require(args.count == 2, "--serve PROJECT_ID"); try serve(settings.profile(args[1]))
    case "--status":
        try require(args.count == 2, "--status PROJECT_ID")
        let p = try settings.profile(args[1]); let pid = jobPID(p.label)
        try jsonOut(["id": p.id, "running": pid != nil, "pid": pid as Any? ?? NSNull(), "url": p.url,
                     "focusFile": p.focusFile, "label": p.label])
    case "--launch-plan":
        try require(args.count == 2, "--launch-plan PROJECT_ID")
        let p = try settings.profile(args[1]); try jsonOut(["arguments": p.arguments(), "environment": p.environment(), "agent": agent(p)])
    case "--agent-plist":
        try require(args.count == 2, "--agent-plist PROJECT_ID")
        let data = try PropertyListSerialization.data(fromPropertyList: agent(settings.profile(args[1])), format: .xml, options: 0)
        FileHandle.standardOutput.write(data)
    case "--login":
        try require(args.count == 2 && ["on", "off"].contains(args[1]), "--login on|off"); try setLogin(args[1] == "on")
    default: throw FlowError(message: "명령: --version, --profiles, --check-config, --control, --status, --launch-plan, --agent-plist, --login")
    }
}
