import AppKit
import SwiftUI

/// Multiline plain-text editor: Return inserts a newline (⌘Return is handled by the menu),
/// Esc calls `onCancel`, grows up to `maxLines` and then scrolls.
struct GrowingTextView: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    var placeholder: String
    var font: NSFont
    var maxLines: Int
    var onCancel: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay

        let tv = PlaceholderTextView()
        tv.placeholder = placeholder
        tv.font = font
        tv.isRichText = false
        tv.importsGraphics = false
        tv.allowsUndo = true
        tv.drawsBackground = false
        tv.isAutomaticQuoteSubstitutionEnabled = false
        tv.isAutomaticDashSubstitutionEnabled = false
        tv.isAutomaticTextReplacementEnabled = false
        // No "Write with Siri" / Writing Tools affordance, no inline predictions.
        tv.writingToolsBehavior = .none
        tv.inlinePredictionType = .no
        tv.textContainerInset = .zero
        tv.textContainer?.lineFragmentPadding = 0
        tv.textContainer?.widthTracksTextView = true
        tv.isVerticallyResizable = true
        tv.isHorizontallyResizable = false
        tv.autoresizingMask = [.width]
        tv.string = text
        tv.delegate = context.coordinator

        scroll.documentView = tv
        context.coordinator.textView = tv
        context.coordinator.observeWindowKey()

        DispatchQueue.main.async {
            tv.window?.makeFirstResponder(tv)
            context.coordinator.recalcHeight()
        }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let tv = context.coordinator.textView else { return }
        if tv.string != text {
            tv.string = text
            tv.needsDisplay = true
        }
        DispatchQueue.main.async { context.coordinator.recalcHeight() }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: GrowingTextView
        weak var textView: NSTextView?
        private var keyObserver: NSObjectProtocol?

        init(_ parent: GrowingTextView) { self.parent = parent }

        deinit {
            if let keyObserver { NotificationCenter.default.removeObserver(keyObserver) }
        }

        func observeWindowKey() {
            keyObserver = NotificationCenter.default.addObserver(
                forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main
            ) { [weak self] note in
                guard let tv = self?.textView, note.object as? NSWindow === tv.window else { return }
                tv.window?.makeFirstResponder(tv)
            }
        }

        func textDidChange(_ notification: Notification) {
            guard let tv = textView else { return }
            parent.text = tv.string
            recalcHeight()
        }

        func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.cancelOperation(_:)) {
                parent.onCancel()
                return true
            }
            return false
        }

        func recalcHeight() {
            guard let tv = textView, let lm = tv.layoutManager, let tc = tv.textContainer else { return }
            lm.ensureLayout(for: tc)
            let line = lm.defaultLineHeight(for: parent.font)
            let used = lm.usedRect(for: tc).height
            let h = ceil(min(max(used, line), line * CGFloat(parent.maxLines)))
            if abs(parent.height - h) > 0.5 { parent.height = h }
        }
    }
}

final class PlaceholderTextView: NSTextView {
    var placeholder = ""

    override func didChangeText() {
        super.didChangeText()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, !placeholder.isEmpty else { return }
        let origin = NSPoint(x: textContainerOrigin.x + (textContainer?.lineFragmentPadding ?? 0),
                             y: textContainerOrigin.y)
        (placeholder as NSString).draw(at: origin, withAttributes: [
            .font: font ?? NSFont.systemFont(ofSize: 13),
            .foregroundColor: NSColor.placeholderTextColor,
        ])
    }
}
