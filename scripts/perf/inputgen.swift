// Minimal synthetic input generator for the perf harness (no cliclick needed).
// Build: swiftc -O inputgen.swift -o inputgen   (bench.sh does this)
// Usage:
//   inputgen move X Y
//   inputgen click X Y
//   inputgen scroll DY COUNT INTERVAL_MS      (negative DY scrolls down)
//   inputgen key KEY [cmd] [shift]            e.g. `key 2 cmd`, `key space`
// Requires Accessibility permission for the terminal that runs it.
import Foundation
import CoreGraphics

func post(_ e: CGEvent?) { e?.post(tap: .cghidEventTap) }

func keyCode(_ k: String) -> CGKeyCode {
    let map: [String: CGKeyCode] = [
        "1": 18, "2": 19, "3": 20, "4": 21, "space": 49, ",": 43, "return": 36, "escape": 53,
        "up": 126, "down": 125, "left": 123, "right": 124,
    ]
    return map[k] ?? 0
}

let args = CommandLine.arguments.dropFirst()
guard let cmd = args.first else { exit(2) }
let a = Array(args.dropFirst())
switch cmd {
case "move":
    let p = CGPoint(x: Double(a[0])!, y: Double(a[1])!)
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
case "click":
    let p = CGPoint(x: Double(a[0])!, y: Double(a[1])!)
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
    usleep(30_000)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left))
    usleep(40_000)
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left))
case "scroll":
    let dy = Int32(a[0])!
    let count = Int(a[1])!
    let interval = UInt32(a.count > 2 ? Int(a[2])! : 16) * 1000
    for _ in 0..<count {
        post(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: dy, wheel2: 0, wheel3: 0))
        usleep(interval)
    }
case "key":
    let code = keyCode(a[0])
    var flags = CGEventFlags()
    if a.contains("cmd") { flags.insert(.maskCommand) }
    if a.contains("shift") { flags.insert(.maskShift) }
    let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)
    let up = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)
    down?.flags = flags
    up?.flags = flags
    post(down)
    usleep(40_000)
    post(up)
default:
    FileHandle.standardError.write("unknown command \(cmd)\n".data(using: .utf8)!)
    exit(2)
}
