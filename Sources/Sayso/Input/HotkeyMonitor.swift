import AppKit
import Carbon.HIToolbox
import os
import SaysoCore

/// 全局快捷键监听（CGEventTap，需要辅助功能权限）。
///
/// Sayso 输入使用单修饰键（如右 ⌥）或「修饰键 + 普通键」组合（如 ⌥T）。监听器只把按键翻译成快捷键，
/// 由 core 按快捷键上命令的输入决定是录音还是读取选区。
/// - 按住模式：快捷键按下即发 `.trigger`，松开发 `.stop`。单修饰键有短暂确认期，确认后才发 `.trigger`。
/// - 按一下模式：组合键按下即发 `.toggle`；单修饰键轻点（很快松开）时发 `.toggle`。
/// - 单修饰键按住期间按了其他键（如 ⌥+X，或 ⌥T 这类组合快捷键），视为在按组合键，本次不触发单修饰键。
/// - 进行中按 Esc 取消；展示结果时按 Esc 关闭候选框。
@MainActor
final class HotkeyMonitor {
    enum Action {
        /// 按住模式下按下快捷键。
        case trigger(Hotkey)
        /// 按住模式下松开快捷键。
        case stop
        /// 按一下模式下按下快捷键。
        case toggle(Hotkey)
        case cancel
        case dismiss(String)
    }

    var style: HotkeyStyle = .hold
    /// 触发已启用命令的快捷键。
    var shortcuts: Set<Hotkey> = []
    /// 录制快捷键时暂停响应。
    var isPaused = false
    var isSessionActive: () -> Bool = { false }
    var candidateSurface: (TimeInterval) -> String? = { _ in nil }
    var onAction: (Action) -> Void = { _ in }

    /// 单修饰键快捷键按下后的确认期，期间没有其他按键才认定为触发。
    private let armDelay: Duration = .milliseconds(150)
    /// 按一下模式下单修饰键视为「轻点」的最长按住时间。
    private let tapThreshold: Duration = .milliseconds(500)

    private var tap: CFMachPort?
    /// 按住中的单修饰键快捷键。同一时刻只会按住一个，状态只需一份。
    private var heldModifier: Hotkey?
    private var modifierDownAt: ContinuousClock.Instant = .now
    /// 单修饰键按住期间按了别的键，本次视为普通组合键。
    private var interrupted = false
    /// 按住模式下已发出 `.trigger`、松开时要发 `.stop` 的键。
    private var triggeredKeys: Set<UInt16> = []
    /// 已吞掉按下事件的组合快捷键主键，松开时一并吞掉。
    private var swallowedKeys: Set<UInt16> = []
    private var armTask: Task<Void, Never>?
    /// 只记录快捷键、Esc 与监听状态，不记录其他按键内容。
    private let logger = Logger(subsystem: "com.sayso.app", category: "hotkey")
    /// 交给 core 的操作在主线程同步执行，超过此时长会拖住事件监听，记录下来以便排查。
    private let slowActionThreshold: Duration = .milliseconds(200)

    var isRunning: Bool { tap != nil }

    @discardableResult
    func start() -> Bool {
        guard tap == nil else { return true }
        let mask = (1 << CGEventType.keyDown.rawValue)
            | (1 << CGEventType.keyUp.rawValue)
            | (1 << CGEventType.flagsChanged.rawValue)
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: CGEventMask(mask),
            callback: hotkeyTapCallback,
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else { return false }

        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        self.tap = tap
        return true
    }

    /// 返回 nil 表示吞掉事件。
    fileprivate func handle(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            // 被禁用期间的按键（如松开快捷键、Esc）不会再送达，状态可能停在按住中。
            logger.error("event tap disabled by \(type == .tapDisabledByTimeout ? "timeout" : "user input", privacy: .public), modifierHeld=\(self.heldModifier != nil)")
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return Unmanaged.passUnretained(event)
        }
        guard !isPaused else { return Unmanaged.passUnretained(event) }

        let keyCode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
        let isRepeat = event.getIntegerValueField(.keyboardEventAutorepeat) != 0
        switch type {
        case .flagsChanged:
            handleFlagsChanged(keyCode: keyCode, flags: event.flags)
        case .keyDown:
            if keyCode == UInt16(kVK_Escape) {
                let active = isSessionActive()
                logger.notice("escape sessionActive=\(active)")
                if active {
                    perform(.cancel)
                    return nil
                }
                // 关闭候选框时吞掉 Esc，避免同时触发前台应用的退出全屏、关闭弹窗等行为。
                if !isRepeat, let surface = dismissibleSurface(for: event) {
                    perform(.dismiss(surface))
                    return nil
                }
            }
            if swallowedKeys.contains(keyCode) { return nil }  // 组合快捷键的自动重复
            let combination = Hotkey(keyCode: keyCode, modifiers: event.flags.intersection(Hotkey.standardModifierMask).rawValue)
            if !isRepeat, !combination.isModifierOnly, shortcuts.contains(combination) {
                combinationDown(combination)
                return nil
            }
            // 非激活面板收不到本地按键；全局监听只通知关闭，Enter 继续交给前台输入框。
            if !isRepeat, heldModifier == nil, (keyCode == UInt16(kVK_Return) || keyCode == UInt16(kVK_ANSI_KeypadEnter)),
               event.flags.intersection(Hotkey.standardModifierMask).isEmpty,
               let surface = dismissibleSurface(for: event) {
                perform(.dismiss(surface))
            }
            if heldModifier != nil { interrupt() }
        case .keyUp:
            if swallowedKeys.remove(keyCode) != nil {
                release(keyCode)
                return nil
            }
        default:
            break
        }
        return Unmanaged.passUnretained(event)
    }

    /// CGEvent 时间戳为纳秒，候选框按秒比较，以排除它出现前产生的按键。
    private func dismissibleSurface(for event: CGEvent) -> String? {
        candidateSurface(TimeInterval(event.timestamp) / 1_000_000_000)
    }

    private func handleFlagsChanged(keyCode: UInt16, flags: CGEventFlags) {
        guard let flag = Hotkey.modifierFlagsByKeyCode[keyCode] else { return }
        let pressed = flags.contains(flag)
        if let held = heldModifier {
            if !pressed, keyCode == held.keyCode {
                modifierUp(held)
            } else if pressed {
                // 按住单修饰键期间又按了其他修饰键，说明用户在按组合快捷键。
                interrupt()
            }
            return
        }
        let candidate = Hotkey(keyCode: keyCode, modifiers: 0)
        // 同时按着其他修饰键时不算单修饰键快捷键。
        guard pressed, shortcuts.contains(candidate), flags.intersection(Hotkey.standardModifierMask.subtracting(flag)).isEmpty else { return }
        modifierDown(candidate)
    }

    private func modifierDown(_ key: Hotkey) {
        logger.notice("modifier shortcut down \(key.displayName, privacy: .public)")
        heldModifier = key
        modifierDownAt = .now
        interrupted = false
        guard style == .hold else { return }
        armTask?.cancel()
        armTask = Task { [weak self, armDelay] in
            try? await Task.sleep(for: armDelay)
            guard let self, !Task.isCancelled, self.heldModifier == key, !self.interrupted else { return }
            self.triggeredKeys.insert(key.keyCode)
            self.perform(.trigger(key))
        }
    }

    private func modifierUp(_ key: Hotkey) {
        logger.notice("modifier shortcut up interrupted=\(self.interrupted)")
        heldModifier = nil
        armTask?.cancel()
        switch style {
        case .hold:
            release(key.keyCode)
        case .toggle:
            guard !interrupted, ContinuousClock.now - modifierDownAt < tapThreshold else { return }
            perform(.toggle(key))
        }
    }

    private func combinationDown(_ key: Hotkey) {
        logger.notice("combination shortcut down \(key.displayName, privacy: .public)")
        // 组合快捷键用到的修饰键不再作为单修饰键快捷键触发。
        if heldModifier != nil { interrupt() }
        swallowedKeys.insert(key.keyCode)
        switch style {
        case .hold:
            triggeredKeys.insert(key.keyCode)
            perform(.trigger(key))
        case .toggle:
            perform(.toggle(key))
        }
    }

    /// 按住模式下松开已触发的键：结束录音。
    private func release(_ keyCode: UInt16) {
        if triggeredKeys.remove(keyCode) != nil { perform(.stop) }
    }

    /// 已触发后（如录音中按了 ⌘C）不再算作打断，按键照常放行。
    private func interrupt() {
        guard let held = heldModifier, !triggeredKeys.contains(held.keyCode) else { return }
        logger.notice("modifier shortcut interrupted by another key")
        interrupted = true
        armTask?.cancel()
    }

    /// 交给 core 执行，并记录阻塞主线程过久的调用。
    private func perform(_ action: Action) {
        let start = ContinuousClock.now
        onAction(action)
        let elapsed = ContinuousClock.now - start
        if elapsed > slowActionThreshold {
            logger.error("action \(String(describing: action), privacy: .public) blocked the main thread for \(elapsed, privacy: .public)")
        }
    }
}

private func hotkeyTapCallback(
    proxy: CGEventTapProxy,
    type: CGEventType,
    event: CGEvent,
    userInfo: UnsafeMutableRawPointer?
) -> Unmanaged<CGEvent>? {
    guard let userInfo else { return Unmanaged.passUnretained(event) }
    let monitor = Unmanaged<HotkeyMonitor>.fromOpaque(userInfo).takeUnretainedValue()
    // 事件源挂在主线程 RunLoop 上，回调必然在主线程执行。
    return MainActor.assumeIsolated { monitor.handle(type: type, event: event) }
}
