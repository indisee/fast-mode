import AppKit
import Foundation

struct HistoryItem: Codable, Identifiable, Equatable {
    var id = UUID()
    var date: Date
    var request: String
    var response: String
}

enum Storage {
    static let dir: URL = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let url = base.appendingPathComponent("FastMode", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }()
    static var systemPromptURL: URL { dir.appendingPathComponent("system-prompt.md") }
    static var historyURL: URL { dir.appendingPathComponent("history.json") }

    static func ensureSystemPrompt() {
        if !FileManager.default.fileExists(atPath: systemPromptURL.path) {
            try? defaultSystemPrompt.write(to: systemPromptURL, atomically: true, encoding: .utf8)
        }
    }

    static func loadHistory() -> [HistoryItem] {
        guard let data = try? Data(contentsOf: historyURL) else { return [] }
        let dec = JSONDecoder()
        dec.dateDecodingStrategy = .iso8601
        return (try? dec.decode([HistoryItem].self, from: data)) ?? []
    }

    static func saveHistory(_ items: [HistoryItem]) {
        let enc = JSONEncoder()
        enc.dateEncodingStrategy = .iso8601
        enc.outputFormatting = [.prettyPrinted]
        if let data = try? enc.encode(items) {
            try? data.write(to: historyURL, options: .atomic)
        }
    }
}

enum Pasteboard {
    static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

@MainActor
final class AppModel: ObservableObject {
    enum Status: Equatable {
        case idle, running, copied
        case error(String)
    }

    static let models: [(label: String, value: String)] = [
        ("Default", ""), ("Haiku", "haiku"), ("Sonnet", "sonnet"), ("Opus", "opus"),
    ]

    @Published var input = ""
    @Published var status: Status = .idle
    @Published var lastResponse: String?
    @Published var history: [HistoryItem] = []
    @Published var showHistory = false
    @Published var model: String = UserDefaults.standard.string(forKey: "model") ?? "" {
        didSet { UserDefaults.standard.set(model, forKey: "model") }
    }
    /// Window opacity while Fast Mode is not focused.
    @Published var inactiveOpacity: Double = (UserDefaults.standard.object(forKey: "inactiveOpacity") as? Double) ?? 0.7 {
        didSet {
            UserDefaults.standard.set(inactiveOpacity, forKey: "inactiveOpacity")
            onOpacityChange?()
        }
    }
    var onOpacityChange: (() -> Void)?

    private var task: Task<Void, Never>?
    private var resetTask: Task<Void, Never>?
    private let runner = ClaudeRunner()
    private let historyLimit = 300
    var openSettings: (() -> Void)?

    init() {
        Storage.ensureSystemPrompt()
        history = Storage.loadHistory()
    }

    var isRunning: Bool { status == .running }

    /// Submits `text` if given (e.g. pasted straight in), otherwise the current input.
    func submit(text override: String? = nil) {
        let snapshot = input
        let text = (override ?? snapshot).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !isRunning else { return }
        resetTask?.cancel()
        status = .running
        let modelName = model
        task = Task {
            do {
                let raw = try await runner.run(input: text, model: modelName)
                let out = Self.stripOuterFence(raw)
                Pasteboard.copy(out)
                lastResponse = out
                history.insert(HistoryItem(date: .now, request: text, response: out), at: 0)
                if history.count > historyLimit { history.removeLast(history.count - historyLimit) }
                Storage.saveHistory(history)
                // Don't wipe something the user typed while waiting.
                if input == snapshot { input = "" }
                flash(.copied)
            } catch is CancellationError {
                status = .idle
            } catch {
                // Keep a pasted request so it isn't lost.
                if input.isEmpty { input = text }
                status = .error(error.localizedDescription)
            }
        }
    }

    func cancel() {
        task?.cancel()
    }

    func copyLast() {
        guard let lastResponse else { return }
        Pasteboard.copy(lastResponse)
        flash(.copied)
    }

    func delete(_ item: HistoryItem) {
        history.removeAll { $0.id == item.id }
        Storage.saveHistory(history)
    }

    func clearHistory() {
        history = []
        Storage.saveHistory(history)
    }

    func editSystemPrompt() {
        let url = Storage.systemPromptURL
        if let textEdit = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.TextEdit") {
            NSWorkspace.shared.open([url], withApplicationAt: textEdit, configuration: NSWorkspace.OpenConfiguration())
        } else {
            NSWorkspace.shared.open(url)
        }
    }

    private func flash(_ s: Status) {
        status = s
        resetTask?.cancel()
        resetTask = Task {
            try? await Task.sleep(for: .seconds(2.5))
            if !Task.isCancelled, status == s { status = .idle }
        }
    }

    /// The prompt asks for markdown to be wrapped in ``` for chat UIs; the clipboard wants the raw text.
    static func stripOuterFence(_ s: String) -> String {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t.hasPrefix("```"), t.hasSuffix("```"), t.count > 6 else { return t }
        var lines = t.components(separatedBy: "\n")
        guard lines.count >= 2 else { return t }
        lines.removeFirst()
        lines.removeLast()
        return lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
