// Simplified to Traditional Chinese by the system's ICU (Hans-Hant), one line in, one line out. scripts/make_hant.py
// runs it to build the converter tables; nothing ships it.
import Foundation

let transform = StringTransform(rawValue: "Hans-Hant")
while let line = readLine(strippingNewline: true) {
    print(line.applyingTransform(transform, reverse: false) ?? line)
}
