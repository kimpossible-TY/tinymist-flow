import AppKit
import UniformTypeIdentifiers

final class FlowApp: NSObject, NSApplicationDelegate {
    var settings = Settings()
    var statusItem: NSStatusItem!
    var window: NSWindow!
    let picker = NSPopUpButton()
    let status = NSTextField(labelWithString: "프로젝트를 선택하세요")
    let detail = NSTextField(wrappingLabelWithString: "")
    var fields: [String: NSTextField] = [:]
    let login = NSButton(checkboxWithTitle: "로그인 시 tinymist-flow 앱 열기", target: nil, action: nil)
    var timer: Timer?
    var active: Profile? { settings.profiles.first { $0.id == settings.selectedID } }
    func applicationDidFinishLaunching(_ note: Notification) {
        do { settings = try loadSettings() } catch { alert(error) }
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem.button?.image = NSImage(systemSymbolName: "wind", accessibilityDescription: "tinymist-flow")
        statusItem.button?.title = " Flow"
        statusItem.button?.imagePosition = .imageLeading
        buildMenu(); buildWindow(); populate()
        if !CommandLine.arguments.contains("--background") { showWindow() }
        timer = Timer.scheduledTimer(withTimeInterval: 8, repeats: true) { [weak self] _ in self?.refresh() }
        refresh()
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool { showWindow(); return true }
    func buildMenu() {
        let menu = NSMenu()
        for (title, selector, key) in [
            ("tinymist-flow 열기…", #selector(showWindow), ","),
            ("프리뷰 열기", #selector(openPreview), "o"),
            ("선택한 프로젝트 시작", #selector(start), ""),
            ("선택한 프로젝트 중지", #selector(stop), ""),
            ("선택한 프로젝트 재시작", #selector(restart), ""),
            ("로그 보기", #selector(openLogs), ""),
            ("사용·업데이트·복구 안내", #selector(guide), ""),
            ("앱 정보", #selector(about), ""),
            ("앱 종료 (프리뷰 계속 실행)", #selector(quit), "q")
        ] { let item = NSMenuItem(title: title, action: selector, keyEquivalent: key); item.target = self; menu.addItem(item) }
        statusItem.menu = menu
        let main = NSMenu(); let app = NSMenuItem(); app.submenu = menu.copy() as? NSMenu; main.addItem(app)
        let editItem = NSMenuItem(); let edit = NSMenu(title: "Edit")
        for (title, action, key) in [("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            edit.addItem(withTitle: title, action: Selector(action), keyEquivalent: key)
        }
        editItem.submenu = edit; main.addItem(editItem); NSApp.mainMenu = main
    }
    func button(_ title: String, _ selector: Selector) -> NSButton { NSButton(title: title, target: self, action: selector) }
    func buildWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 660), styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = appName; window.isReleasedWhenClosed = false; window.center()
        let content = NSStackView(); content.orientation = .vertical; content.alignment = .leading; content.spacing = 14
        content.edgeInsets = NSEdgeInsets(top: 24, left: 28, bottom: 24, right: 28)
        content.translatesAutoresizingMaskIntoConstraints = false
        window.contentView!.addSubview(content)
        NSLayoutConstraint.activate([content.leadingAnchor.constraint(equalTo: window.contentView!.leadingAnchor), content.trailingAnchor.constraint(equalTo: window.contentView!.trailingAnchor), content.topAnchor.constraint(equalTo: window.contentView!.topAnchor)])
        let icon = NSImageView(); icon.image = NSImage(named: NSImage.applicationIconName); icon.widthAnchor.constraint(equalToConstant: 64).isActive = true; icon.heightAnchor.constraint(equalToConstant: 64).isActive = true
        let title = NSTextField(labelWithString: "tinymist-flow"); title.font = .systemFont(ofSize: 26, weight: .semibold)
        let sub = NSTextField(labelWithString: "문서에서 프리뷰까지, 한 흐름으로."); sub.textColor = .secondaryLabelColor
        let heading = NSStackView(views: [title, sub]); heading.orientation = .vertical; heading.alignment = .leading
        content.addArrangedSubview(NSStackView(views: [icon, heading]))
        picker.target = self; picker.action = #selector(selectProfile); picker.widthAnchor.constraint(equalToConstant: 400).isActive = true
        content.addArrangedSubview(NSStackView(views: [picker, button("프로젝트 추가…", #selector(addProject))]))
        status.font = .systemFont(ofSize: 14, weight: .medium); content.addArrangedSubview(status)
        detail.textColor = .secondaryLabelColor; detail.font = .systemFont(ofSize: 12); content.addArrangedSubview(detail)
        content.addArrangedSubview(NSStackView(views: [button("프리뷰 열기", #selector(openPreview)), button("주소 복사", #selector(copyURL)), button("시작", #selector(start)), button("중지", #selector(stop)), button("재시작", #selector(restart)), button("로그", #selector(openLogs))]))
        let definitions = [("name", "프로젝트 이름"), ("root", "프로젝트 폴더"), ("entry", "진입 파일"), ("fonts", "글꼴 폴더 ( ; 구분)"), ("packages", "로컬 패키지 폴더"), ("port", "로컬 포트"), ("publicURL", "HTTPS 주소 (선택)")]
        let grid = NSGridView(views: definitions.map { key, label in
            let field = NSTextField(); field.setAccessibilityLabel(label); field.widthAnchor.constraint(equalToConstant: 440).isActive = true; fields[key] = field
            return [NSTextField(labelWithString: label), field]
        }); grid.rowSpacing = 10; grid.columnSpacing = 12; grid.xPlacement = .leading
        content.addArrangedSubview(grid)
        let hint = NSTextField(wrappingLabelWithString: "글꼴 경로는 프로젝트 폴더 기준입니다. HTTPS 주소는 Tail Hosting에서 연결한 주소를 입력하세요. 저장하면 실행 중인 프리뷰가 재시작됩니다.")
        hint.font = .systemFont(ofSize: 11); hint.textColor = .secondaryLabelColor; hint.widthAnchor.constraint(equalToConstant: 630).isActive = true
        content.addArrangedSubview(hint)
        content.addArrangedSubview(NSStackView(views: [button("설정 저장", #selector(save)), button("설정 파일 보기", #selector(revealSettings))]))
        login.target = self; login.action = #selector(toggleLogin); login.state = loginEnabled() ? .on : .off; content.addArrangedSubview(login)
        let footer = NSTextField(labelWithString: "프리뷰는 시작 후 로그인 시에도 실행됩니다. 중지하면 자동 실행도 해제됩니다.")
        footer.font = .systemFont(ofSize: 11); footer.textColor = .secondaryLabelColor; content.addArrangedSubview(footer)
    }
    func populate() {
        picker.removeAllItems(); picker.addItems(withTitles: settings.profiles.map(\.name))
        if let index = settings.profiles.firstIndex(where: { $0.id == settings.selectedID }) { picker.selectItem(at: index) }
        else if let first = settings.profiles.first { settings.selectedID = first.id; picker.selectItem(at: 0) }
        guard let p = active else { fields.values.forEach { $0.stringValue = "" }; return }
        for (key, value) in ["name": p.name, "root": p.root, "entry": p.entry, "fonts": p.fonts.joined(separator: "; "), "packages": p.packages, "port": String(p.port), "publicURL": p.publicURL] { fields[key]?.stringValue = value }
        refresh()
    }
    func refresh() {
        guard let p = active else { status.stringValue = "프로젝트를 추가해 시작하세요"; detail.stringValue = ""; return }
        guard let pid = jobPID(p.label) else { status.stringValue = "중지됨"; detail.stringValue = p.url; return }
        status.stringValue = "실행 중 · PID \(pid)"; detail.stringValue = p.url
        var request = URLRequest(url: URL(string: "http://127.0.0.1:\(p.port)/")!); request.timeoutInterval = 2
        URLSession.shared.dataTask(with: request) { [weak self] _, response, error in
            DispatchQueue.main.async {
                guard let self, self.active?.id == p.id, jobPID(p.label) == pid else { return }
                let ok = (response as? HTTPURLResponse)?.statusCode == 200
                self.status.stringValue = ok ? "프리뷰 준비됨 · PID \(pid)" : "시작 중 또는 접근 권한 확인 필요 · PID \(pid)"
                if error != nil { self.detail.stringValue = "문서 접근 권한과 로그를 확인하세요.  " + p.url }
            }
        }.resume()
    }
    func alert(_ error: Error) { let a = NSAlert(); a.messageText = "tinymist-flow"; a.informativeText = error.localizedDescription; a.runModal() }
    @objc func showWindow() { window?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true) }
    @objc func selectProfile() { guard picker.indexOfSelectedItem >= 0 else { return }; settings.selectedID = settings.profiles[picker.indexOfSelectedItem].id; do { try saveSettings(settings); populate() } catch { alert(error) } }
    @objc func addProject() {
        let panel = NSOpenPanel(); panel.title = "Typst 진입 파일 선택"; panel.allowedContentTypes = [UTType(filenameExtension: "typ")!]; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let file = panel.url else { return }
        let root = file.deletingLastPathComponent()
        let ports = Set(settings.profiles.map(\.port)); let port = (23625...65535).first { !ports.contains($0) }!
        let id = "project-" + UUID().uuidString.prefix(8).lowercased()
        let p = Profile(id: id, name: root.lastPathComponent, root: root.path, entry: file.lastPathComponent,
                        fonts: fm.fileExists(atPath: root.appendingPathComponent("fonts").path) ? ["fonts"] : [],
                        packages: userHome.appendingPathComponent("Library/Application Support/typst/packages").path,
                        port: port, publicURL: "", label: bundleID + ".preview." + id)
        settings.profiles.append(p); settings.selectedID = id
        do { try saveSettings(settings); populate() } catch { alert(error) }
    }
    @objc func save() {
        guard let old = active, let index = settings.profiles.firstIndex(where: { $0.id == old.id }) else { return }
        do {
            func value(_ key: String) -> String { fields[key]!.stringValue.trimmingCharacters(in: .whitespacesAndNewlines) }
            guard let port = Int(value("port")) else { throw FlowError(message: "포트를 숫자로 입력하세요.") }
            var updated = old; updated.name = value("name"); updated.root = value("root"); updated.entry = value("entry")
            updated.fonts = value("fonts").split(separator: ";").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            updated.packages = value("packages"); updated.port = port; updated.publicURL = value("publicURL")
            try updated.validate(checkFiles: true)
            var next = settings; next.profiles[index] = updated; try next.validate()
            let running = jobPID(old.label) != nil
            if running { try control("stop", old) }
            do { try saveSettings(next); if running { try control("start", updated) } }
            catch { try? saveSettings(settings); if running { try? control("start", old) }; throw error }
            settings = next; populate()
        } catch { alert(error) }
    }
    func perform(_ action: String) { guard let p = active else { return }; do { try control(action, p); refresh() } catch { alert(error) } }
    @objc func start() { perform("start") }
    @objc func stop() { perform("stop") }
    @objc func restart() { perform("restart") }
    @objc func openPreview() { if let p = active, let url = URL(string: p.url) { NSWorkspace.shared.open(url) } }
    @objc func copyURL() { if let p = active { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(p.url, forType: .string) } }
    @objc func openLogs() {
        guard let p = active else { return }
        let file = logRoot.appendingPathComponent("\(p.id).err.log")
        if fm.fileExists(atPath: file.path) { NSWorkspace.shared.open(file) } else { NSWorkspace.shared.open(logRoot) }
    }
    @objc func revealSettings() { NSWorkspace.shared.activateFileViewerSelecting([support.appendingPathComponent("profiles.json")]) }
    @objc func toggleLogin() { do { try setLogin(login.state == .on) } catch { login.state = loginEnabled() ? .on : .off; alert(error) } }
    @objc func about() { NSApp.orderFrontStandardAboutPanel(options: [.applicationName: appName, .credits: NSAttributedString(string: "Personal Typst workspace. Powered by Tinymist and Typst. Apache License 2.0.\nInstalled releases and recovery: see the bundled User Guide.")]) }
    @objc func guide() { if let url = Bundle.main.url(forResource: "UserGuide", withExtension: "pdf") { NSWorkspace.shared.open(url) } }
    @objc func quit() { NSApp.terminate(nil) }
}

if CommandLine.arguments.count > 1 && CommandLine.arguments[1] != "--background" {
    do { try runCLI(Array(CommandLine.arguments.dropFirst())) } catch { FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8)); exit(1) }
} else {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let delegate = FlowApp(); app.delegate = delegate
    app.run()
}
