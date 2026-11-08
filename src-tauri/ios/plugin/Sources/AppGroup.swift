import Foundation

enum AppGroup {
    static let identifier = "group.io.github.xraybox"
    static let memoryBudget: UInt64 = 45 * 1024 * 1024

    static func container() -> URL? {
        let url = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: identifier
        )
        if let url {
            try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            try? FileManager.default.setAttributes(
                [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
                ofItemAtPath: url.path
            )
        }
        return url
    }

    static func file(_ name: String) -> URL? {
        container()?.appendingPathComponent(name)
    }

    static func exists(_ name: String) -> Bool {
        guard let url = file(name) else { return false }
        return FileManager.default.fileExists(atPath: url.path)
    }

    static func writeAtomic(_ name: String, data: Data) throws {
        guard let url = file(name), let base = container() else {
            throw CocoaError(.fileNoSuchFile)
        }
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        let tmp = base.appendingPathComponent("\(name).tmp")
        try data.write(to: tmp, options: .atomic)
        if FileManager.default.fileExists(atPath: url.path) {
            try FileManager.default.removeItem(at: url)
        }
        try FileManager.default.moveItem(at: tmp, to: url)
        try? FileManager.default.setAttributes(
            [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
            ofItemAtPath: url.path
        )
    }
}

struct TunnelSession: Codable {
    var configPath: String
    var assetDir: String
    var logPath: String
    var serverHost: String
    var serverPort: UInt16
    var excludeHosts: [String]
    var dns: [String]
    var killSwitch: Bool
    var socksPort: UInt16
    var onDemand: Bool

    static func load() -> TunnelSession? {
        guard let url = AppGroup.file("session.json"),
              let data = try? Data(contentsOf: url) else {
            return nil
        }
        return try? JSONDecoder().decode(TunnelSession.self, from: data)
    }

    func save() {
        guard let data = try? JSONEncoder().encode(self) else { return }
        try? AppGroup.writeAtomic("session.json", data: data)
    }
}

struct ReadyFile: Codable {
    var ok: Bool
    var footprint: UInt64
    var error: String
}

struct StatsFile: Codable {
    var uplink: UInt64
    var downlink: UInt64
}
