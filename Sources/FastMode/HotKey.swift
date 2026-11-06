import Carbon.HIToolbox
import Foundation

/// System-wide hotkey via Carbon's RegisterEventHotKey (needs no Accessibility permission).
/// One shared Carbon handler; `register` replaces whatever was registered before.
enum GlobalHotKey {
    private static var ref: EventHotKeyRef?
    private static var handlerInstalled = false
    private static var action: (() -> Void)?

    /// Returns false if the combination couldn't be registered (usually: owned by another app).
    @discardableResult
    static func register(keyCode: Int, modifiers: Int, action: @escaping () -> Void) -> Bool {
        unregister()
        installHandlerIfNeeded()
        self.action = action
        let id = EventHotKeyID(signature: OSType(0x464D_4F44) /* 'FMOD' */, id: 1)
        let status = RegisterEventHotKey(UInt32(keyCode), UInt32(modifiers), id,
                                         GetEventDispatcherTarget(), 0, &ref)
        NSLog("FastMode: register hotkey code=\(keyCode) mods=\(modifiers) status=\(status)")
        return status == noErr
    }

    static func unregister() {
        if let ref { UnregisterEventHotKey(ref) }
        ref = nil
    }

    private static func installHandlerIfNeeded() {
        guard !handlerInstalled else { return }
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        let status = InstallEventHandler(GetEventDispatcherTarget(), { _, _, _ in
            NSLog("FastMode: hotkey pressed")
            DispatchQueue.main.async { GlobalHotKey.action?() }
            return noErr
        }, 1, &spec, nil, nil)
        handlerInstalled = status == noErr
        NSLog("FastMode: install handler status=\(status)")
    }
}
