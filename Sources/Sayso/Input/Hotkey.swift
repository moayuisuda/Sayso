import AppKit
import Carbon.HIToolbox
import SaysoCore

/// 快捷键。可以是单个修饰键（如右 Option），也可以是「修饰键 + 普通键」组合。
/// 键码与修饰位（`CGEventFlags` 中的标准修饰位）由 core 原样保存，这里提供 macOS 上的解释与显示。
extension Hotkey {
    static let rightOption = Hotkey(keyCode: UInt16(kVK_RightOption), modifiers: 0)
    static let optionT = Hotkey(keyCode: UInt16(kVK_ANSI_T), modifiers: CGEventFlags.maskAlternate.rawValue)

    static let standardModifierMask: CGEventFlags = [.maskCommand, .maskShift, .maskAlternate, .maskControl]

    /// 修饰键 keyCode 与其对应的标志位。
    static let modifierFlagsByKeyCode: [UInt16: CGEventFlags] = [
        UInt16(kVK_Command): .maskCommand, UInt16(kVK_RightCommand): .maskCommand,
        UInt16(kVK_Shift): .maskShift, UInt16(kVK_RightShift): .maskShift,
        UInt16(kVK_Option): .maskAlternate, UInt16(kVK_RightOption): .maskAlternate,
        UInt16(kVK_Control): .maskControl, UInt16(kVK_RightControl): .maskControl,
        UInt16(kVK_Function): .maskSecondaryFn,
    ]

    var isModifierOnly: Bool { Self.modifierFlagsByKeyCode[keyCode] != nil }

    var isFunctionKey: Bool { Self.functionKeyNumber(keyCode) != nil }

    var requiredFlags: CGEventFlags { CGEventFlags(rawValue: modifiers) }

    /// 逐个按键的显示名，用于键帽展示：修饰键按 ⌃⌥⇧⌘ 顺序在前，主键在后；单修饰键热键只有一个键。
    var keycaps: [String] {
        if isModifierOnly { return [Self.modifierKeyName(keyCode)] }
        let symbols: [(CGEventFlags, String)] = [(.maskControl, "⌃"), (.maskAlternate, "⌥"), (.maskShift, "⇧"), (.maskCommand, "⌘")]
        return symbols.filter { requiredFlags.contains($0.0) }.map(\.1) + [Self.keyName(keyCode)]
    }

    var displayName: String { keycaps.joined() }

    private static func modifierKeyName(_ keyCode: UInt16) -> String {
        switch Int(keyCode) {
        case kVK_RightOption: return L("右 ⌥", "Right ⌥")
        case kVK_Option: return L("左 ⌥", "Left ⌥")
        case kVK_RightCommand: return L("右 ⌘", "Right ⌘")
        case kVK_Command: return L("左 ⌘", "Left ⌘")
        case kVK_RightShift: return L("右 ⇧", "Right ⇧")
        case kVK_Shift: return L("左 ⇧", "Left ⇧")
        case kVK_RightControl: return L("右 ⌃", "Right ⌃")
        case kVK_Control: return L("左 ⌃", "Left ⌃")
        default: return "fn"
        }
    }

    /// 普通键的显示名（按当前键盘布局）。
    static func keyName(_ keyCode: UInt16) -> String {
        if let number = functionKeyNumber(keyCode) { return "F\(number)" }
        switch Int(keyCode) {
        case kVK_Space: return "Space"
        case kVK_Return: return "↩"
        case kVK_Tab: return "⇥"
        default:
            let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource().takeRetainedValue()
            guard let layoutData = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else {
                return "#\(keyCode)"
            }
            let layout = unsafeBitCast(layoutData, to: CFData.self)
            let keyLayout = unsafeBitCast(CFDataGetBytePtr(layout), to: UnsafePointer<UCKeyboardLayout>.self)
            var deadKeyState: UInt32 = 0
            var chars = [UniChar](repeating: 0, count: 4)
            var length = 0
            UCKeyTranslate(keyLayout, keyCode, UInt16(kUCKeyActionDisplay), 0, UInt32(LMGetKbdType()),
                           OptionBits(kUCKeyTranslateNoDeadKeysBit), &deadKeyState, chars.count, &length, &chars)
            return length > 0 ? String(utf16CodeUnits: chars, count: length).uppercased() : "#\(keyCode)"
        }
    }

    private static func functionKeyNumber(_ keyCode: UInt16) -> Int? {
        let map: [Int: Int] = [
            kVK_F1: 1, kVK_F2: 2, kVK_F3: 3, kVK_F4: 4, kVK_F5: 5, kVK_F6: 6, kVK_F7: 7, kVK_F8: 8,
            kVK_F9: 9, kVK_F10: 10, kVK_F11: 11, kVK_F12: 12, kVK_F13: 13, kVK_F14: 14, kVK_F15: 15,
            kVK_F16: 16, kVK_F17: 17, kVK_F18: 18, kVK_F19: 19, kVK_F20: 20,
        ]
        return map[Int(keyCode)]
    }
}

extension HotkeyStyle: Identifiable {
    static let allCases: [HotkeyStyle] = [.hold, .toggle]

    public var id: Self { self }

    var displayName: String {
        switch self {
        case .hold: return L("按住说话", "Hold to talk")
        case .toggle: return L("按一下开始/结束", "Press to start/stop")
        }
    }
}
