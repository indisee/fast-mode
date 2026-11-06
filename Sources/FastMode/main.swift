import AppKit
import Carbon.HIToolbox
import SwiftUI

final class FloatingPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
    var onCancel: (() -> Void)?
    /// Esc when no text view has focus (e.g. while "Thinking…") — cancel, never close.
    override func cancelOperation(_ sender: Any?) { onCancel?() }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var panel: FloatingPanel!
    private let model = AppModel()

    static let compactWidth: CGFloat = 70
    static let expandedWidth: CGFloat = 420

    /// Where the user placed the window (its top-left). Growth starts from here and
    /// shifts up/left only when it would run off screen; shrinking returns to it.
    private var home: NSPoint = .zero
    private var isProgrammaticMove = false
    private let homeKey = "FastModeHomeTopLeft"

    /// Accessory apps have no menu bar, but key equivalents (⌘V, ⌘↩…) are still routed through the main menu.
    private func installMainMenu() {
        let main = NSMenu()

        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        let send = appMenu.addItem(withTitle: "Send", action: #selector(sendRequest(_:)), keyEquivalent: "\r")
        send.target = self
        let settings = appMenu.addItem(withTitle: "Settings…", action: #selector(openSettings(_:)), keyEquivalent: ",")
        settings.target = self
        appMenu.addItem(withTitle: "Quit Fast Mode", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let editItem = NSMenuItem()
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        let paste = edit.addItem(withTitle: "Paste", action: #selector(smartPaste(_:)), keyEquivalent: "v")
        paste.target = self
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)

        NSApp.mainMenu = main
    }

    private let shortcut = ShortcutStore()
    private var settingsWindow: NSWindow?

    /// Global shortcut (default ⌃⌥V): send the clipboard without stealing focus from the current app.
    private func registerHotKey() {
        guard shortcut.isSet else {
            GlobalHotKey.unregister()
            shortcut.conflict = false
            return
        }
        let ok = GlobalHotKey.register(keyCode: shortcut.keyCode, modifiers: shortcut.modifiers) { [weak self] in
            self?.sendClipboard()
        }
        shortcut.conflict = !ok
    }

    @objc func openSettings(_ sender: Any? = nil) {
        if settingsWindow == nil {
            let w = NSWindow(contentViewController: NSHostingController(
                rootView: SettingsView(model: model, shortcut: shortcut)))
            w.title = "Fast Mode Settings"
            w.styleMask = [.titled, .closable]
            w.level = .floating
            w.isReleasedWhenClosed = false
            w.center()
            settingsWindow = w
        }
        NSApp.activate()
        settingsWindow?.makeKeyAndOrderFront(nil)
    }

    private func sendClipboard() {
        guard !model.isRunning,
              let text = NSPasteboard.general.string(forType: .string),
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { NSSound.beep(); return }
        panel.orderFrontRegardless()
        model.submit(text: text)
    }

    @objc private func sendRequest(_ sender: Any?) {
        model.submit()
    }

    /// ⌘V into an empty field sends the clipboard right away; otherwise it's a normal paste.
    @objc private func smartPaste(_ sender: Any?) {
        if NSApp.keyWindow === panel, model.input.isEmpty, !model.isRunning,
           let text = NSPasteboard.general.string(forType: .string),
           !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            model.submit(text: text)
        } else {
            NSApp.sendAction(#selector(NSText.paste(_:)), to: nil, from: sender)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        installMainMenu()
        shortcut.onChange = { [weak self] in self?.registerHotKey() }
        shortcut.onRecording = { [weak self] recording in
            guard let self else { return }
            if recording { GlobalHotKey.unregister() } else { registerHotKey() }
        }
        model.openSettings = { [weak self] in self?.openSettings() }
        model.onOpacityChange = { [weak self] in self?.updateOpacity() }
        registerHotKey()
        panel = FloatingPanel(
            contentRect: NSRect(x: 0, y: 0, width: Self.compactWidth, height: 70),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = false
        panel.isMovableByWindowBackground = true
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = true
        panel.delegate = self
        panel.onCancel = { [weak self] in self?.model.cancel() }

        let root = ContentView(model: model) { [weak self] size in
            self?.resize(to: size)
        }
        let host = NSHostingView(rootView: root)
        host.sizingOptions = []
        panel.contentView = host

        if let saved = UserDefaults.standard.string(forKey: homeKey) {
            home = NSPointFromString(saved)
        } else if let vf = NSScreen.main?.visibleFrame {
            home = NSPoint(x: vf.midX - Self.compactWidth / 2, y: vf.maxY - 80)
        }
        applyFrame(size: panel.frame.size)
        panel.makeKeyAndOrderFront(nil)
        NSApp.activate()
        updateOpacity()
        debugSnapshotIfRequested()
    }

    /// Dev aid: FASTMODE_SNAPSHOT=/path.png [FASTMODE_STATUS=running|copied|error] [FASTMODE_LAST=1]
    /// renders the window into a PNG (no screen-recording permission needed) and quits.
    private func debugSnapshotIfRequested() {
        let env = ProcessInfo.processInfo.environment
        guard let out = env["FASTMODE_SNAPSHOT"] else { return }
        if env["FASTMODE_LAST"] != nil { model.lastResponse = "x" }
        switch env["FASTMODE_STATUS"] {
        case "running": model.status = .running
        case "copied": model.status = .copied
        case "error": model.status = .error("Something went wrong")
        default: break
        }
        if let text = env["FASTMODE_INPUT"] { model.input = text }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [self] in
            guard let view = panel.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { exit(1) }
            view.cacheDisplay(in: view.bounds, to: rep)
            try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: out))
            exit(0)
        }
    }

    private func resize(to size: CGSize) {
        guard panel != nil, size.width > 0, size.height > 0 else { return }
        applyFrame(size: size)
    }

    private func applyFrame(size: CGSize) {
        var frame = NSRect(x: home.x, y: home.y - size.height, width: size.width, height: size.height)
        let screen = NSScreen.screens.first { $0.frame.contains(home) } ?? panel.screen ?? NSScreen.main
        if let vf = screen?.visibleFrame {
            // Not enough room below → grow upward (bottom-anchored); same for the right edge.
            if frame.minY < vf.minY { frame.origin.y = vf.minY }
            if frame.maxY > vf.maxY { frame.origin.y = vf.maxY - frame.height }
            if frame.maxX > vf.maxX { frame.origin.x = max(vf.minX, vf.maxX - frame.width) }
        }
        guard frame != panel.frame else { return }
        isProgrammaticMove = true
        panel.setFrame(frame, display: true, animate: false)
        isProgrammaticMove = false
        panel.invalidateShadow()
    }

    private func updateOpacity() {
        guard let panel else { return }
        panel.alphaValue = panel.isKeyWindow ? 1 : CGFloat(model.inactiveOpacity)
    }

    func windowDidBecomeKey(_ notification: Notification) { updateOpacity() }
    func windowDidResignKey(_ notification: Notification) { updateOpacity() }

    func windowDidMove(_ notification: Notification) {
        guard !isProgrammaticMove, let panel else { return }
        home = NSPoint(x: panel.frame.minX, y: panel.frame.maxY)
        UserDefaults.standard.set(NSStringFromPoint(home), forKey: homeKey)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        panel.makeKeyAndOrderFront(nil)
        return true
    }
}

MainActor.assumeIsolated {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    app.setActivationPolicy(.accessory)
    app.run()
}
