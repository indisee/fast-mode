import AppKit
import Carbon.HIToolbox
import SwiftUI

/// Persisted global shortcut (Carbon key code + Carbon modifier flags).
@MainActor
final class ShortcutStore: ObservableObject {
    @Published private(set) var keyCode: Int
    @Published private(set) var modifiers: Int
    @Published private(set) var label: String
    /// Registration failed — another app owns this combination.
    @Published var conflict = false

    /// Called after the shortcut changes, and with `true`/`false` while recording
    /// (so the old hotkey doesn't fire mid-recording).
    var onChange: (() -> Void)?
    var onRecording: ((Bool) -> Void)?

    private let d = UserDefaults.standard

    init() {
        if d.object(forKey: "hotKeyCode") != nil {
            keyCode = d.integer(forKey: "hotKeyCode")
            modifiers = d.integer(forKey: "hotKeyMods")
            label = d.string(forKey: "hotKeyLabel") ?? ""
        } else {
            keyCode = kVK_ANSI_V
            modifiers = controlKey | optionKey
            label = "⌃⌥V"
        }
    }

    var isSet: Bool { keyCode >= 0 }

    func set(keyCode: Int, modifiers: Int, label: String) {
        self.keyCode = keyCode
        self.modifiers = modifiers
        self.label = label
        d.set(keyCode, forKey: "hotKeyCode")
        d.set(modifiers, forKey: "hotKeyMods")
        d.set(label, forKey: "hotKeyLabel")
        onChange?()
    }

    func clear() { set(keyCode: -1, modifiers: 0, label: "") }

    // MARK: Event → Carbon / label

    static func carbonModifiers(_ f: NSEvent.ModifierFlags) -> Int {
        var m = 0
        if f.contains(.command) { m |= cmdKey }
        if f.contains(.option) { m |= optionKey }
        if f.contains(.control) { m |= controlKey }
        if f.contains(.shift) { m |= shiftKey }
        return m
    }

    static func label(flags f: NSEvent.ModifierFlags, keyCode: Int, fallback: String?) -> String {
        var s = ""
        if f.contains(.control) { s += "⌃" }
        if f.contains(.option) { s += "⌥" }
        if f.contains(.shift) { s += "⇧" }
        if f.contains(.command) { s += "⌘" }
        return s + (keyNames[keyCode] ?? fallback?.uppercased() ?? "?")
    }

    /// Layout-independent names (hotkeys bind to physical keys, so a Russian layout shouldn't show "М").
    private static let keyNames: [Int: String] = {
        var m: [Int: String] = [
            kVK_Space: "Space", kVK_Return: "↩", kVK_Tab: "⇥", kVK_Delete: "⌫", kVK_ForwardDelete: "⌦",
            kVK_LeftArrow: "←", kVK_RightArrow: "→", kVK_UpArrow: "↑", kVK_DownArrow: "↓",
            kVK_Home: "↖", kVK_End: "↘", kVK_PageUp: "⇞", kVK_PageDown: "⇟",
            kVK_ANSI_Minus: "-", kVK_ANSI_Equal: "=", kVK_ANSI_LeftBracket: "[", kVK_ANSI_RightBracket: "]",
            kVK_ANSI_Semicolon: ";", kVK_ANSI_Quote: "'", kVK_ANSI_Comma: ",", kVK_ANSI_Period: ".",
            kVK_ANSI_Slash: "/", kVK_ANSI_Backslash: "\\", kVK_ANSI_Grave: "`",
        ]
        let letters: [(Int, String)] = [
            (kVK_ANSI_A, "A"), (kVK_ANSI_B, "B"), (kVK_ANSI_C, "C"), (kVK_ANSI_D, "D"), (kVK_ANSI_E, "E"),
            (kVK_ANSI_F, "F"), (kVK_ANSI_G, "G"), (kVK_ANSI_H, "H"), (kVK_ANSI_I, "I"), (kVK_ANSI_J, "J"),
            (kVK_ANSI_K, "K"), (kVK_ANSI_L, "L"), (kVK_ANSI_M, "M"), (kVK_ANSI_N, "N"), (kVK_ANSI_O, "O"),
            (kVK_ANSI_P, "P"), (kVK_ANSI_Q, "Q"), (kVK_ANSI_R, "R"), (kVK_ANSI_S, "S"), (kVK_ANSI_T, "T"),
            (kVK_ANSI_U, "U"), (kVK_ANSI_V, "V"), (kVK_ANSI_W, "W"), (kVK_ANSI_X, "X"), (kVK_ANSI_Y, "Y"),
            (kVK_ANSI_Z, "Z"), (kVK_ANSI_0, "0"), (kVK_ANSI_1, "1"), (kVK_ANSI_2, "2"), (kVK_ANSI_3, "3"),
            (kVK_ANSI_4, "4"), (kVK_ANSI_5, "5"), (kVK_ANSI_6, "6"), (kVK_ANSI_7, "7"), (kVK_ANSI_8, "8"),
            (kVK_ANSI_9, "9"),
            (kVK_F1, "F1"), (kVK_F2, "F2"), (kVK_F3, "F3"), (kVK_F4, "F4"), (kVK_F5, "F5"), (kVK_F6, "F6"),
            (kVK_F7, "F7"), (kVK_F8, "F8"), (kVK_F9, "F9"), (kVK_F10, "F10"), (kVK_F11, "F11"), (kVK_F12, "F12"),
        ]
        for (k, v) in letters { m[k] = v }
        return m
    }()
}

struct ShortcutRecorder: View {
    @ObservedObject var store: ShortcutStore
    @State private var recording = false
    @State private var monitor: Any?

    var body: some View {
        HStack(spacing: 6) {
            Button(action: { recording ? stop() : start() }) {
                Text(recording ? "Type shortcut…" : (store.isSet ? store.label : "Record Shortcut"))
                    .font(.system(size: 12, weight: store.isSet && !recording ? .medium : .regular))
                    .foregroundStyle(recording ? .secondary : .primary)
                    .frame(minWidth: 110)
            }
            if store.isSet, !recording {
                Button { store.clear() } label: { Image(systemName: "xmark.circle.fill") }
                    .buttonStyle(.plain)
                    .foregroundStyle(.tertiary)
                    .help("Remove shortcut")
            }
        }
        .onDisappear { stop() }
    }

    private func start() {
        recording = true
        store.onRecording?(true)
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let flags = event.modifierFlags.intersection([.command, .option, .control, .shift])
            let code = Int(event.keyCode)
            if code == kVK_Escape, flags.isEmpty { stop(); return nil }
            if code == kVK_Delete, flags.isEmpty { store.clear(); stop(); return nil }
            guard !flags.intersection([.command, .option, .control]).isEmpty else {
                NSSound.beep() // need at least ⌘, ⌥ or ⌃
                return nil
            }
            store.set(
                keyCode: code,
                modifiers: ShortcutStore.carbonModifiers(flags),
                label: ShortcutStore.label(flags: flags, keyCode: code, fallback: event.charactersIgnoringModifiers)
            )
            stop()
            return nil
        }
    }

    private func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        if recording {
            recording = false
            store.onRecording?(false)
        }
    }
}

struct SettingsView: View {
    @ObservedObject var model: AppModel
    @ObservedObject var shortcut: ShortcutStore

    var body: some View {
        Form {
            Section {
                LabeledContent("Send clipboard to Claude") {
                    ShortcutRecorder(store: shortcut)
                }
            } footer: {
                VStack(alignment: .leading, spacing: 4) {
                    if shortcut.conflict {
                        Text("This shortcut is taken by another app — pick a different one.")
                            .foregroundStyle(.red)
                    }
                    Text("Works from any app. Click, then press a combination with ⌘, ⌥ or ⌃. Esc cancels, ⌫ removes.")
                        .foregroundStyle(.secondary)
                }
                .font(.system(size: 11))
            }
            Section {
                Picker("Model", selection: $model.model) {
                    ForEach(AppModel.models, id: \.value) { Text($0.label).tag($0.value) }
                }
                LabeledContent("Opacity when not focused") {
                    HStack(spacing: 8) {
                        Slider(value: $model.inactiveOpacity, in: 0.2...1.0, step: 0.05)
                            .frame(width: 140)
                        Text("\(Int((model.inactiveOpacity * 100).rounded()))%")
                            .monospacedDigit()
                            .frame(width: 36, alignment: .trailing)
                    }
                }
                LabeledContent("System prompt") {
                    Button("Edit…", action: model.editSystemPrompt)
                }
            }
        }
        .formStyle(.grouped)
        .font(.system(size: 12))
        .frame(width: 400)
        .fixedSize(horizontal: false, vertical: true)
    }
}
