// Selects a macOS input source by id, so the Vietnamese gate can be driven
// without touching the input menu by hand.
//
//   swift scripts/select_input_source.swift com.apple.inputmethod.VietnameseTelex
//   swift scripts/select_input_source.swift com.apple.keylayout.ABC
import Carbon
import Foundation

let target =
    CommandLine.arguments.count > 1
    ? CommandLine.arguments[1] : "com.apple.inputmethod.VietnameseTelex"

guard let list = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else {
    print("no input sources")
    exit(1)
}

for source in list {
    guard let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { continue }
    let id = Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    if id == target {
        let status = TISSelectInputSource(source)
        print("selected \(id) status=\(status)")
        exit(status == noErr ? 0 : 1)
    }
}

print("not found: \(target)")
exit(1)
