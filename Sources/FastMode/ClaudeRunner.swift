import Foundation

struct ClaudeError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

final class ClaudeRunner: @unchecked Sendable {
    private static let extraPaths = [
        "\(NSHomeDirectory())/.local/bin",
        "\(NSHomeDirectory())/.claude/local",
        "/opt/homebrew/bin",
        "/usr/local/bin",
    ]

    private lazy var executable: String? = Self.resolveClaude()

    /// GUI apps don't inherit the shell PATH, so look in the usual places, then ask a login shell.
    private static func resolveClaude() -> String? {
        for dir in extraPaths {
            let p = "\(dir)/claude"
            if FileManager.default.isExecutableFile(atPath: p) { return p }
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/zsh")
        p.arguments = ["-lc", "command -v claude"]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return nil }
        p.waitUntilExit()
        let path = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return path.isEmpty ? nil : path
    }

    func run(input: String, model: String) async throws -> String {
        guard let exe = executable else {
            throw ClaudeError(message: "claude CLI not found")
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: exe)
        var args = [
            "-p",
            "--system-prompt-file", Storage.systemPromptURL.path,
            "--tools", "",
            "--output-format", "text",
        ]
        if !model.isEmpty { args += ["--model", model] }
        process.arguments = args
        // Neutral cwd so no project CLAUDE.md gets pulled in.
        process.currentDirectoryURL = Storage.dir
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = (Self.extraPaths + [env["PATH"] ?? "/usr/bin:/bin"]).joined(separator: ":")
        process.environment = env

        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (cont: CheckedContinuation<String, Error>) in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        try process.run()
                    } catch {
                        cont.resume(throwing: ClaudeError(message: "Failed to launch claude: \(error.localizedDescription)"))
                        return
                    }
                    stdin.fileHandleForWriting.write(Data(input.utf8))
                    try? stdin.fileHandleForWriting.close()

                    var errData = Data()
                    let group = DispatchGroup()
                    group.enter()
                    DispatchQueue.global().async {
                        errData = stderr.fileHandleForReading.readDataToEndOfFile()
                        group.leave()
                    }
                    let outData = stdout.fileHandleForReading.readDataToEndOfFile()
                    group.wait()
                    process.waitUntilExit()

                    if process.terminationReason == .uncaughtSignal {
                        cont.resume(throwing: CancellationError())
                        return
                    }
                    let out = String(decoding: outData, as: UTF8.self)
                        .trimmingCharacters(in: .whitespacesAndNewlines)
                    if process.terminationStatus != 0 || out.isEmpty {
                        let err = String(decoding: errData, as: UTF8.self)
                            .trimmingCharacters(in: .whitespacesAndNewlines)
                        let msg = [err, out].first { !$0.isEmpty } ?? "claude exited with code \(process.terminationStatus)"
                        cont.resume(throwing: ClaudeError(message: msg))
                    } else {
                        cont.resume(returning: out)
                    }
                }
            }
        } onCancel: {
            if process.isRunning { process.terminate() }
        }
    }
}
