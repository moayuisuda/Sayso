import AppKit
import Carbon.HIToolbox
import SaysoCore

/// 全局热键监听（CGEventTap，需要辅助功能权限）。
///
/// 所有命令都从统一入口键触发，按住入口键期间按的路由键决定交给哪些命令。监听器只负责把按键翻译成路由，
/// 由会话按路由上命令的输入决定是录音还是读取选区。
/// - 按住模式：入口键确认按下即发 `.route(.direct)`，按住期间每按一个路由键发 `.route(.key)`，松开发 `.stop`。
///   单修饰键入口有短暂确认期，期间按了路由键则直接发该路由，不再发 `.direct`。
/// - 按一下模式：松开入口键时发 `.toggle`，附带按住期间按的路由键（没按则为 `.direct`）。
/// - 入口键按住期间按了其他修饰键或非路由键（如 ⌥+X 快捷键），视为普通快捷键放行，本次不触发。
/// - 进行中按 Esc 取消。
@MainActor
final class HotkeyMonitor {
    enum Action {
        /// 按住模式下触发一个路由。
        case route(RouteKey)
        /// 按住模式下松开入口键。
        case stop
        /// 按一下模式下轻点入口键，附带本次的路由。
        case toggle([RouteKey])
        case cancel
    }

    var style: HotkeyStyle = .hold
    var trigger: Hotkey = .rightOption
    /// 已启用命令绑定的路由键。
    var routeKeys: Set<UInt16> = []
    /// 录制快捷键时暂停响应。
    var isPaused = false
    var isSessionActive: () -> Bool = { false }
    var onAction: (Action) -> Void = { _ in }

    /// 单修饰键入口按下后的确认期，期间没有其他按键才认定为直接触发。
    private let armDelay: Duration = .milliseconds(150)
    /// 按一下模式下视为「轻点」的最长按住时间；按了路由键时不受限。
    private let tapThreshold: Duration = .milliseconds(500)

    private var tap: CFMachPort?
    // 本次按住入口键的状态。同一时刻只会按住一次入口键，状态只需一份。
    private var isTriggerHeld = false
    private var triggerDownAt: ContinuousClock.Instant = .now
    /// 按住期间按了别的键，本次视为普通快捷键。
    private var interrupted = false
    /// 按住模式下本次已发出路由，松开时需要发 `.stop`。
    private var routed = false
    /// 按一下模式下本次按的路由键，松开时一并发出。
    private var pressedRoutes: [RouteKey] = []
    /// 已吞掉按下事件的路由键，松开时一并吞掉。
    private var swallowedKeys: Set<UInt16> = []
    private var armTask: Task<Void, Never>?

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
            if keyCode == UInt16(kVK_Escape), isSessionActive() {
                onAction(.cancel)
                return nil
            }
            // 组合键入口：按下即按住。
            if !trigger.isModifierOnly, !isTriggerHeld, keyCode == trigger.keyCode,
               event.flags.intersection(Hotkey.standardModifierMask) == trigger.requiredFlags {
                triggerDown(interrupted: false)
                return nil
            }
            guard isTriggerHeld else { break }
            if keyCode == trigger.keyCode { return nil }  // 组合键入口的自动重复
            if !interrupted, routeKeys.contains(keyCode) {
                if !isRepeat { routeKeyDown(keyCode) }
                swallowedKeys.insert(keyCode)
                return nil
            }
            interrupt()
        case .keyUp:
            if swallowedKeys.remove(keyCode) != nil { return nil }
            if !trigger.isModifierOnly, isTriggerHeld, keyCode == trigger.keyCode {
                triggerUp()
                return nil
            }
        default:
            break
        }
        return Unmanaged.passUnretained(event)
    }

    private func handleFlagsChanged(keyCode: UInt16, flags: CGEventFlags) {
        guard trigger.isModifierOnly, keyCode == trigger.keyCode else {
            // 按住入口键期间又按了其他修饰键，说明用户在按组合快捷键。
            if isTriggerHeld, Hotkey.modifierFlagsByKeyCode[keyCode] != nil { interrupt() }
            return
        }
        guard let flag = Hotkey.modifierFlagsByKeyCode[keyCode] else { return }
        let pressed = flags.contains(flag)
        if pressed, !isTriggerHeld {
            triggerDown(interrupted: !flags.intersection(Hotkey.standardModifierMask.subtracting(flag)).isEmpty)
        } else if !pressed, isTriggerHeld {
            triggerUp()
        }
    }

    private func triggerDown(interrupted: Bool) {
        isTriggerHeld = true
        triggerDownAt = .now
        self.interrupted = interrupted
        routed = false
        pressedRoutes = []
        guard style == .hold, !interrupted else { return }
        if trigger.isModifierOnly { armDirect() } else { send(.direct) }
    }

    private func routeKeyDown(_ keyCode: UInt16) {
        switch style {
        case .hold:
            armTask?.cancel()
            send(.key(code: keyCode))
        case .toggle:
            pressedRoutes.append(.key(code: keyCode))
        }
    }

    private func triggerUp() {
        isTriggerHeld = false
        armTask?.cancel()
        switch style {
        case .hold:
            if routed { onAction(.stop) }
        case .toggle:
            guard !interrupted, !pressedRoutes.isEmpty || ContinuousClock.now - triggerDownAt < tapThreshold else { return }
            onAction(.toggle(pressedRoutes.isEmpty ? [.direct] : pressedRoutes))
        }
    }

    /// 已触发后（如录音中按了 ⌘C）不再算作打断，按键照常放行。
    private func interrupt() {
        guard !routed else { return }
        interrupted = true
        armTask?.cancel()
    }

    private func armDirect() {
        armTask?.cancel()
        armTask = Task { [weak self, armDelay] in
            try? await Task.sleep(for: armDelay)
            guard let self, !Task.isCancelled, self.isTriggerHeld, !self.interrupted, !self.routed else { return }
            self.send(.direct)
        }
    }

    private func send(_ route: RouteKey) {
        routed = true
        onAction(.route(route))
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
