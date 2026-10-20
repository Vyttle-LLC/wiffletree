import CryptoKit
import Foundation

func bytes(_ hex: String) -> Data {
    var data = Data(); var i = hex.startIndex
    while i < hex.endIndex { let j = hex.index(i, offsetBy: 2); data.append(UInt8(hex[i..<j], radix: 16)!); i = j }
    return data
}
func hex(_ d: some Sequence<UInt8>) -> String { d.map { String(format: "%02x", $0) }.joined() }
let url = URL(fileURLWithPath: CommandLine.arguments[1])
let v = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: [String: String]]
let transcript = bytes(v["transcript"]!["bytes"]!)
func proof(_ key: Data, _ label: String) -> String {
    hex(HMAC<SHA256>.authenticationCode(for: Data(label.utf8) + transcript, using: SymmetricKey(data: key)))
}
let qr = bytes(v["qr"]!["secret"]!)
let code = bytes(v["code"]!["key"]!)
let checks = [
    ("qr device_proof", proof(qr, "device"), v["qr"]!["device_proof"]!),
    ("qr host_proof", proof(qr, "host"), v["qr"]!["host_proof"]!),
    ("code device_confirmation", proof(code, "device"), v["code"]!["device_confirmation"]!),
    ("code host_confirmation", proof(code, "host"), v["code"]!["host_confirmation"]!),
    ("host_id", hex(SHA256.hash(data: bytes(v["keys"]!["host_public"]!))), v["keys"]!["host_id"]!),
]
var ok = true
for (name, got, want) in checks { print(name, got == want ? "matches" : "DIFFERS"); ok = ok && got == want }
let device = try P256.Signing.PublicKey(x963Representation: bytes(v["keys"]!["device_public"]!))
let signature = try P256.Signing.ECDSASignature(rawRepresentation: bytes(v["challenge"]!["signature"]!))
let verified = device.isValidSignature(signature, for: bytes(v["challenge"]!["signed"]!))
print("challenge signature", verified ? "verifies" : "FAILS")
exit(ok && verified ? 0 : 1)
