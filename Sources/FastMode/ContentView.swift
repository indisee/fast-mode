import AppKit
import SwiftUI

/// Transparent area that drags the (borderless) window.
struct WindowDragArea: NSViewRepresentable {
    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
    }
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ nsView: NSView, context: Context) {}
}

enum UI {
    static let textSize: CGFloat = 13
    static let smallSize: CGFloat = 12
    static let iconSize: CGFloat = 11
    static let font = NSFont.systemFont(ofSize: textSize)
}

struct ContentView: View {
    @ObservedObject var model: AppModel
    var onSizeChange: (CGSize) -> Void
    @State private var textHeight: CGFloat = 16

    private var isError: Bool {
        if case .error = model.status { return true }
        return false
    }

    /// Narrow until the user actually types (or opens history).
    private var expanded: Bool {
        !model.input.isEmpty || model.showHistory || isError
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            inputBox
            footer
            if case .error(let msg) = model.status {
                Text(msg)
                    .font(.system(size: UI.smallSize))
                    .foregroundStyle(.red)
                    .lineLimit(3)
                    .textSelection(.enabled)
                    .padding(.horizontal, 2)
            }
            if model.showHistory {
                HistoryView(model: model)
            }
        }
        .padding(8)
        // Compact state is a square; it widens once you type or open history.
        .frame(
            width: expanded ? AppDelegate.expandedWidth : AppDelegate.compactWidth,
            alignment: .topLeading
        )
        .frame(minHeight: expanded ? 0 : AppDelegate.compactWidth, alignment: .top)
        .background(WindowDragArea())
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.1))
        )
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGSize.self) { $0.size } action: { onSizeChange($0) }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onChange(of: model.input) {
            if isError { model.status = .idle }
        }
    }

    // MARK: Input

    private var inputBox: some View {
        GrowingTextView(
            text: $model.input,
            height: $textHeight,
            placeholder: "",
            font: UI.font,
            maxLines: 12,
            onCancel: { model.cancel() }
        )
        .frame(height: textHeight)
        .frame(maxWidth: .infinity, alignment: .leading)
        // In the compact square the field fills the space above the icons.
        .frame(maxHeight: expanded ? nil : .infinity, alignment: .topLeading)
        .padding(.horizontal, 7)
        .padding(.vertical, 6)
        .background(
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(Color.primary.opacity(0.04))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.08))
        )
    }

    private var settingsMenu: some View {
        Menu {
            Picker("Model", selection: $model.model) {
                ForEach(AppModel.models, id: \.value) { Text($0.label).tag($0.value) }
            }
            Divider()
            Button(model.showHistory ? "Hide History" : "History (\(model.history.count))") {
                model.showHistory.toggle()
            }
            Divider()
            Button("Settings…") { model.openSettings?() }
            Button("Edit System Prompt…", action: model.editSystemPrompt)
            Button("Clear History", role: .destructive, action: model.clearHistory)
            Divider()
            Button("Quit Fast Mode") { NSApp.terminate(nil) }
        } label: {
            Image(systemName: "gearshape")
                .font(.system(size: UI.iconSize))
                .foregroundStyle(.secondary)
                .frame(width: 16, height: 16)
                .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Settings")
    }

    // MARK: Footer

    /// settings · status (spinner / check / error) · copy.
    /// Status sits in a ZStack so it's centered whether or not the copy button exists.
    private var footer: some View {
        ZStack {
            HStack(spacing: 0) {
                settingsMenu
                Spacer(minLength: 0) // empty space stays draggable
                if model.lastResponse != nil {
                    Button(action: model.copyLast) {
                        Image(systemName: "doc.on.doc")
                            .font(.system(size: UI.iconSize))
                            .foregroundStyle(.secondary)
                            .frame(width: 16, height: 16)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("Copy last response again")
                }
            }
            statusIcon
                .frame(width: 16, height: 16)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 16)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch model.status {
        case .running:
            ProgressView()
                .controlSize(.small)
                .scaleEffect(0.6)
                .contentShape(Rectangle())
                .onTapGesture { model.cancel() }
                .help("Thinking… Click or press Esc to cancel")
        case .copied:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: UI.iconSize))
                .foregroundStyle(.green)
                .help("Copied")
        case .error(let msg):
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: UI.iconSize))
                .foregroundStyle(.red)
                .help(msg)
        case .idle:
            Color.clear
        }
    }

}
