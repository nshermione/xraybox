import Foundation
import NetworkExtension
import Tauri

struct EstablishArgs: Decodable {
    let killSwitch: Bool
    let onDemand: Bool
}

struct FlagArgs: Decodable {
    let enabled: Bool
}

struct PathResponse: Encodable {
    let path: String
}

struct RunningResponse: Encodable {
    let running: Bool
}

class VpnPlugin: Plugin {
    private let extensionBundle = "io.github.xraybox.PacketTunnel"
    private let stopTimeoutSeconds: TimeInterval = 8
    private let readyTimeoutSeconds: TimeInterval = 30
    private let startIgnoredSeconds: TimeInterval = 10
    private let stopPollSeconds: TimeInterval = 0.1
    private let establishTokenLock = NSLock()
    private var establishToken = 0

    @objc public func sharedDir(_ invoke: Invoke) {
        guard let url = AppGroup.container() else {
            invoke.reject("app group unavailable", code: "app_group_missing")
            return
        }
        invoke.resolve(PathResponse(path: url.path))
    }

    @objc public func prepare(_ invoke: Invoke) {
        DispatchQueue.main.async {
            self.loadManager { result in
                switch result {
                case .success(let manager):
                    self.configure(manager, killSwitch: false, onDemand: false)
                    manager.saveToPreferences { error in
                        if let error {
                            self.rejectConsent(invoke, error: error)
                            return
                        }
                        manager.loadFromPreferences { loadError in
                            if let loadError {
                                self.rejectConsent(invoke, error: loadError)
                            } else {
                                invoke.resolve()
                            }
                        }
                    }
                case .failure(let error):
                    invoke.reject(error.localizedDescription, code: "vpn_establish_failed")
                }
            }
        }
    }

    @objc public func establish(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(EstablishArgs.self)
        let token = advanceEstablishToken()
        DispatchQueue.main.async {
            self.loadManager { result in
                guard self.isCurrentEstablish(token) else {
                    self.rejectSuperseded(invoke)
                    return
                }
                switch result {
                case .failure(let error):
                    invoke.reject(error.localizedDescription, code: "vpn_establish_failed")
                case .success(let manager):
                    // Starting while the previous tunnel is still going down is
                    // silently ignored by iOS, which leaves the app waiting on a
                    // tunnel that never comes up.
                    self.ensureStopped(
                        manager,
                        deadline: Date().addingTimeInterval(self.stopTimeoutSeconds)
                    ) { stopped in
                        guard self.isCurrentEstablish(token) else {
                            self.rejectSuperseded(invoke)
                            return
                        }
                        guard stopped else {
                            invoke.reject(
                                "previous vpn tunnel did not stop",
                                code: "vpn_establish_failed"
                            )
                            return
                        }
                        self.saveAndStart(manager, args: args, invoke: invoke, token: token)
                    }
                }
            }
        }
    }

    private func saveAndStart(
        _ manager: NETunnelProviderManager,
        args: EstablishArgs,
        invoke: Invoke,
        token: Int
    ) {
        configure(manager, killSwitch: args.killSwitch, onDemand: args.onDemand)
        applyOnDemand(manager, enabled: args.onDemand)
        manager.saveToPreferences { error in
            guard self.isCurrentEstablish(token) else {
                self.rejectSuperseded(invoke)
                return
            }
            if let error {
                self.rejectConsent(invoke, error: error)
                return
            }
            manager.loadFromPreferences { loadError in
                guard self.isCurrentEstablish(token) else {
                    self.rejectSuperseded(invoke)
                    return
                }
                if let loadError {
                    self.rejectConsent(invoke, error: loadError)
                    return
                }
                do {
                    try manager.connection.startVPNTunnel()
                } catch {
                    invoke.reject(error.localizedDescription, code: "vpn_establish_failed")
                    return
                }
                self.waitUntilReady(
                    manager: manager,
                    invoke: invoke,
                    sawActivity: false,
                    started: Date(),
                    token: token
                )
            }
        }
    }

    @objc public func teardown(_ invoke: Invoke) {
        _ = advanceEstablishToken()
        DispatchQueue.main.async {
            self.loadManager { result in
                guard case .success(let manager) = result else {
                    invoke.resolve()
                    return
                }
                self.applyOnDemand(manager, enabled: false)
                // Resolve only once the tunnel is down, so a quick reconnect
                // never races the extension that is still shutting down.
                self.ensureStopped(
                    manager,
                    deadline: Date().addingTimeInterval(self.stopTimeoutSeconds)
                ) { _ in
                    manager.saveToPreferences { _ in
                        invoke.resolve()
                    }
                }
            }
        }
    }

    @objc public func running(_ invoke: Invoke) {
        DispatchQueue.main.async {
            self.loadManager { result in
                let status = (try? result.get())?.connection.status
                let up = status == .connected || status == .reasserting
                invoke.resolve(RunningResponse(running: up))
            }
        }
    }

    @objc public func setKillSwitch(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(FlagArgs.self)
        if var session = TunnelSession.load() {
            session.killSwitch = args.enabled
            session.save()
        }
        DispatchQueue.main.async {
            self.loadManager { result in
                if case .success(let manager) = result,
                   let proto = manager.protocolConfiguration as? NETunnelProviderProtocol {
                    proto.enforceRoutes = args.enabled
                    manager.protocolConfiguration = proto
                    manager.saveToPreferences { _ in
                        self.send(["op": "killSwitch", "enabled": args.enabled], invoke: invoke)
                    }
                    return
                }
                self.send(["op": "killSwitch", "enabled": args.enabled], invoke: invoke)
            }
        }
    }

    @objc public func reload(_ invoke: Invoke) {
        send(["op": "reload"], invoke: invoke)
    }

    @objc public func setOnDemand(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(FlagArgs.self)
        DispatchQueue.main.async {
            self.loadManager { result in
                guard case .success(let manager) = result else {
                    invoke.resolve()
                    return
                }
                self.applyOnDemand(manager, enabled: args.enabled)
                manager.saveToPreferences { error in
                    if let error {
                        invoke.reject(error.localizedDescription, code: "vpn_establish_failed")
                    } else {
                        invoke.resolve()
                    }
                }
            }
        }
    }

    private func waitUntilReady(
        manager: NETunnelProviderManager,
        invoke: Invoke,
        sawActivity: Bool,
        started: Date,
        token: Int
    ) {
        guard isCurrentEstablish(token) else {
            rejectSuperseded(invoke)
            return
        }
        let elapsed = Date().timeIntervalSince(started)
        if elapsed > readyTimeoutSeconds {
            invoke.reject(
                "xray started but the extension did not become ready",
                code: "vpn_establish_failed"
            )
            return
        }
        let status = manager.connection.status
        if !sawActivity && elapsed > startIgnoredSeconds && (status == .disconnected || status == .invalid) {
            invoke.reject("vpn tunnel never started", code: "vpn_establish_failed")
            return
        }
        if let ready = readReady() {
            if ready.ok && ready.footprint > AppGroup.memoryBudget {
                invoke.reject("ios_memory_budget", code: "ios_memory_budget")
                return
            }
            if ready.ok && (status == .connected || status == .reasserting) {
                invoke.resolve()
                return
            }
            if !ready.ok {
                let code = ready.error.contains("ios_memory_budget") ? "ios_memory_budget" : "vpn_establish_failed"
                invoke.reject(ready.error.isEmpty ? "vpn_establish_failed" : ready.error, code: code)
                return
            }
        }
        let active = sawActivity || status == .connecting || status == .reasserting || status == .connected
        if active && (status == .disconnected || status == .invalid) {
            invoke.reject("vpn_establish_failed", code: "vpn_establish_failed")
            return
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { [weak self] in
            self?.waitUntilReady(
                manager: manager,
                invoke: invoke,
                sawActivity: active,
                started: started,
                token: token
            )
        }
    }

    /// Every establish and teardown advances the token, so an older establish
    /// that is still polling learns it was superseded and gives up.
    private func advanceEstablishToken() -> Int {
        establishTokenLock.lock()
        defer { establishTokenLock.unlock() }
        establishToken += 1
        return establishToken
    }

    private func isCurrentEstablish(_ token: Int) -> Bool {
        establishTokenLock.lock()
        defer { establishTokenLock.unlock() }
        return token == establishToken
    }

    /// An invoke must always be answered, or the Rust caller blocks forever and
    /// the app can no longer connect.
    private func rejectSuperseded(_ invoke: Invoke) {
        invoke.reject("vpn_cancelled", code: "vpn_cancelled")
    }

    private func ensureStopped(
        _ manager: NETunnelProviderManager,
        deadline: Date,
        completion: @escaping (Bool) -> Void
    ) {
        let status = manager.connection.status
        if status == .disconnected || status == .invalid {
            completion(true)
            return
        }
        if Date() >= deadline {
            completion(false)
            return
        }
        if status != .disconnecting {
            manager.connection.stopVPNTunnel()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + stopPollSeconds) { [weak self] in
            guard let self else {
                completion(false)
                return
            }
            self.ensureStopped(manager, deadline: deadline, completion: completion)
        }
    }

    private func readReady() -> ReadyFile? {
        guard let url = AppGroup.file("ready.json"),
              let data = try? Data(contentsOf: url) else {
            return nil
        }
        return try? JSONDecoder().decode(ReadyFile.self, from: data)
    }

    private func send(_ body: [String: Any], invoke: Invoke) {
        guard let data = try? JSONSerialization.data(withJSONObject: body) else {
            invoke.resolve()
            return
        }
        DispatchQueue.main.async {
            self.loadManager { result in
                guard case .success(let manager) = result,
                      let session = manager.connection as? NETunnelProviderSession,
                      manager.connection.status == .connected else {
                    invoke.resolve()
                    return
                }
                try? session.sendProviderMessage(data) { _ in
                    invoke.resolve()
                }
            }
        }
    }

    private func loadManager(completion: @escaping (Result<NETunnelProviderManager, Error>) -> Void) {
        NETunnelProviderManager.loadAllFromPreferences { managers, error in
            if let error {
                completion(.failure(error))
                return
            }
            let manager = managers?.first(where: { item in
                let proto = item.protocolConfiguration as? NETunnelProviderProtocol
                return proto?.providerBundleIdentifier == self.extensionBundle
            }) ?? NETunnelProviderManager()
            completion(.success(manager))
        }
    }

    private func configure(_ manager: NETunnelProviderManager, killSwitch: Bool, onDemand: Bool) {
        let proto = (manager.protocolConfiguration as? NETunnelProviderProtocol) ?? NETunnelProviderProtocol()
        proto.providerBundleIdentifier = extensionBundle
        proto.serverAddress = "xraybox"
        proto.includeAllNetworks = true
        proto.excludeLocalNetworks = true
        proto.enforceRoutes = killSwitch
        proto.disconnectOnSleep = false
        _ = onDemand
        manager.protocolConfiguration = proto
        manager.localizedDescription = "xraybox"
        manager.isEnabled = true
    }

    private func applyOnDemand(_ manager: NETunnelProviderManager, enabled: Bool) {
        if enabled {
            let rule = NEOnDemandRuleConnect()
            rule.interfaceTypeMatch = .any
            manager.onDemandRules = [rule]
            manager.isOnDemandEnabled = true
        } else {
            manager.onDemandRules = []
            manager.isOnDemandEnabled = false
        }
    }

    private func rejectConsent(_ invoke: Invoke, error: Error) {
        let ns = error as NSError
        let text = ns.localizedDescription.lowercased()
        let denied = text.contains("cancel")
            || text.contains("denied")
            || text.contains("not allowed")
            || (ns.domain == "NEVPNErrorDomain" && (ns.code == 5 || ns.code == 6))
        if denied {
            invoke.reject("vpn_denied", code: "vpn_denied")
        } else {
            invoke.reject(ns.localizedDescription, code: "vpn_establish_failed")
        }
    }
}

@_cdecl("init_plugin_xraybox_vpn")
func initPluginXrayboxVpn() -> Plugin {
    VpnPlugin()
}
