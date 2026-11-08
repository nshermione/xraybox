import Darwin
import Foundation
import Network
import NetworkExtension

final class PacketTunnelProvider: NEPacketTunnelProvider {
    private var session = TunnelSession.load()
    private var hev: HevTunnel?
    private var tunFd: Int32 = -1
    private var pathMonitor: NWPathMonitor?
    private var lastInterface: NWInterface.InterfaceType?
    private var watchdog: DispatchSourceTimer?
    private let xrayQueue = DispatchQueue(label: "io.github.xraybox.xray")
    private var killSwitch = false
    private var started = false
    private var lastStats: StatsFile?
    // Resource logging is off. Uncomment these, `logResources()` below, and the
    // helpers at the bottom of this file to track CPU and memory again.
    // private var resourceLogTick = 0
    // private var lastResources: ResourceSample?

    override func startTunnel(
        options: [String: NSObject]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        _ = options
        if AppGroup.exists("user-stop") {
            completionHandler(tunnelError("user-stop"))
            return
        }
        loadSession(deadline: Date().addingTimeInterval(sessionWaitSeconds)) { [weak self] session in
            guard let self else { return }
            guard let session else {
                completionHandler(tunnelError("missing session.json"))
                return
            }
            self.bringUp(session, completionHandler: completionHandler)
        }
    }

    /// App Group files are `completeUntilFirstUserAuthentication`. On Demand can
    /// start the tunnel after a reboot before the phone has been unlocked once,
    /// so wait for the container to become readable instead of failing outright.
    private func loadSession(deadline: Date, completion: @escaping (TunnelSession?) -> Void) {
        if let session = TunnelSession.load() {
            completion(session)
            return
        }
        if Date() >= deadline {
            completion(nil)
            return
        }
        xrayQueue.asyncAfter(deadline: .now() + 1) { [weak self] in
            guard let self else {
                completion(nil)
                return
            }
            self.loadSession(deadline: deadline, completion: completion)
        }
    }

    private func bringUp(
        _ session: TunnelSession,
        completionHandler: @escaping (Error?) -> Void
    ) {
        self.session = session
        killSwitch = session.killSwitch
        clearReady()
        let settings = networkSettings(session: session, addresses: resolve(session.excludeHosts))
        setTunnelNetworkSettings(settings) { [weak self] error in
            guard let self else { return }
            if let error {
                completionHandler(error)
                return
            }
            let fd = xraybox_utun_fd()
            if fd < 0 {
                self.writeReady(ok: false, error: "utun fd not found")
                completionHandler(tunnelError("utun fd not found"))
                return
            }
            self.tunFd = fd
            let startError = self.startXray(session)
            if let startError {
                self.writeReady(ok: false, error: startError)
                completionHandler(tunnelError(startError))
                return
            }
            self.hev = HevTunnel(tunFd: fd, socksPort: session.socksPort)
            let footprint = physicalFootprint()
            if footprint > AppGroup.memoryBudget {
                self.hev?.stop()
                self.hev = nil
                self.stopXray()
                self.writeReady(ok: false, footprint: footprint, error: "ios_memory_budget")
                completionHandler(tunnelError("ios_memory_budget"))
                return
            }
            self.writeReady(ok: true, footprint: footprint, error: "")
            self.started = true
            self.startWatchdog()
            self.startPathMonitor()
            completionHandler(nil)
        }
    }

    override func stopTunnel(
        with reason: NEProviderStopReason,
        completionHandler: @escaping () -> Void
    ) {
        _ = reason
        started = false
        watchdog?.cancel()
        watchdog = nil
        pathMonitor?.cancel()
        pathMonitor = nil
        hev?.stop()
        hev = nil
        stopXray()
        clearReady()
        completionHandler()
    }

    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        guard let object = try? JSONSerialization.jsonObject(with: messageData) as? [String: Any],
              let op = object["op"] as? String else {
            completionHandler?(nil)
            return
        }
        switch op {
        case "killSwitch":
            let enabled = object["enabled"] as? Bool ?? false
            killSwitch = enabled
            session?.killSwitch = enabled
            session?.save()
            reapplySettings()
            if !enabled && !xrayIsRunning() {
                cancelTunnelWithError(tunnelError("xray stopped"))
            }
        case "reload":
            clearReady()
            guard let fresh = TunnelSession.load() else {
                writeReady(ok: false, error: "missing session.json")
                completionHandler?(nil)
                return
            }
            session = fresh
            killSwitch = fresh.killSwitch
            if let error = restartXray(fresh) {
                writeReady(ok: false, error: error)
            } else {
                writeReady(ok: true, error: "")
                reapplySettings()
            }
            completionHandler?(nil)
            return
        case "status":
            let payload: [String: Any] = [
                "running": xrayIsRunning(),
                "footprint": physicalFootprint(),
            ]
            completionHandler?(try? JSONSerialization.data(withJSONObject: payload))
            return
        default:
            break
        }
        completionHandler?(nil)
    }

    override func sleep(completionHandler: @escaping () -> Void) {
        completionHandler()
    }

    override func wake() {
        guard started, let session else { return }
        if !xrayIsRunning() {
            recover(session)
        }
    }

    private func startXray(_ session: TunnelSession) -> String? {
        var error = [CChar](repeating: 0, count: 512)
        let code = session.configPath.withCString { config in
            session.assetDir.withCString { asset in
                XrayStart(config, asset, &error, Int32(error.count))
            }
        }
        if code == 0 {
            return nil
        }
        return String(cString: error)
    }

    private func stopXray() {
        XrayStop()
    }

    private func xrayIsRunning() -> Bool {
        XrayRunning() == 1
    }

    /// While Xray is down, hev cannot open its SOCKS inbound, so packets are
    /// dropped instead of leaving the tunnel.
    @discardableResult
    private func restartXray(_ session: TunnelSession) -> String? {
        stopXray()
        return startXray(session)
    }

    /// hev died while xray is still up. Without it the tunnel carries nothing,
    /// so rebuild it on the same utun descriptor. A failed rebuild under kill
    /// switch leaves the tunnel in place as a blackhole.
    private func restartPump(_ session: TunnelSession) {
        hev?.stop()
        hev = nil
        guard tunFd >= 0 else {
            NSLog("xraybox hev restart failed: utun fd not found")
            if !killSwitch {
                cancelTunnelWithError(tunnelError("utun fd not found"))
            }
            return
        }
        hev = HevTunnel(tunFd: tunFd, socksPort: session.socksPort)
    }

    private func recover(_ session: TunnelSession) {
        if killSwitch && !AppGroup.exists("user-stop") {
            if let error = restartXray(session) {
                NSLog("xraybox restart failed \(error)")
            }
            return
        }
        cancelTunnelWithError(tunnelError("xray stopped"))
    }

    private func startWatchdog() {
        let timer = DispatchSource.makeTimerSource(queue: xrayQueue)
        timer.schedule(deadline: .now() + 2, repeating: 2)
        timer.setEventHandler { [weak self] in
            guard let self, self.started, let session = self.session else { return }
            self.publishStats()
            // self.logResources()
            if !self.xrayIsRunning() {
                self.recover(session)
                return
            }
            if self.hev?.isHealthy() == false {
                self.restartPump(session)
            }
        }
        timer.resume()
        watchdog = timer
    }

    private func startPathMonitor() {
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            self?.handlePath(path)
        }
        monitor.start(queue: xrayQueue)
        pathMonitor = monitor
    }

    private func handlePath(_ path: Network.NWPath) {
        guard started, path.status == .satisfied, let session else { return }
        let interface = path.availableInterfaces.first?.type
        let changed = interface != lastInterface && lastInterface != nil
        lastInterface = interface
        guard changed else { return }
        let addresses = resolve(session.excludeHosts)
        let settings = networkSettings(session: session, addresses: addresses)
        setTunnelNetworkSettings(settings) { [weak self] _ in
            guard let self else { return }
            _ = self.restartXray(session)
        }
    }

    private func reapplySettings() {
        guard let session else { return }
        let settings = networkSettings(session: session, addresses: resolve(session.excludeHosts))
        setTunnelNetworkSettings(settings, completionHandler: nil)
    }

    private func publishStats() {
        let stats = StatsFile(
            uplink: UInt64(XrayProxyUplink()),
            downlink: UInt64(XrayProxyDownlink())
        )
        if let last = lastStats, last.uplink == stats.uplink, last.downlink == stats.downlink {
            return
        }
        guard let data = try? JSONEncoder().encode(stats) else { return }
        try? AppGroup.writeAtomic("stats.json", data: data)
        lastStats = stats
    }

    /*
    private func logResources() {
        resourceLogTick += 1
        if resourceLogTick % resourceLogEveryTicks != 0 {
            return
        }
        let megabyte = 1024.0 * 1024.0
        let footprint = Double(physicalFootprint()) / megabyte
        let goMemory = Double(XrayGoMemory()) / megabyte
        NSLog(
            "xraybox memory footprint=%.1fMB go=%.1fMB other=%.1fMB",
            footprint,
            goMemory,
            footprint - goMemory
        )
        let sample = ResourceSample.now()
        if let last = lastResources, let line = sample.describe(since: last) {
            NSLog("xraybox cpu %@", line)
        }
        lastResources = sample
    }
    */

    private func networkSettings(session: TunnelSession, addresses: [String]) -> NEPacketTunnelNetworkSettings {
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "172.19.0.1")
        let ipv4 = NEIPv4Settings(addresses: ["172.19.0.2"], subnetMasks: ["255.255.255.252"])
        ipv4.includedRoutes = [NEIPv4Route.default()]
        ipv4.excludedRoutes = addresses.filter { $0.contains(".") }.map { ip in
            NEIPv4Route(destinationAddress: ip, subnetMask: "255.255.255.255")
        }
        settings.ipv4Settings = ipv4

        let ipv6 = NEIPv6Settings(
            addresses: ["fd00:7a72:6179::2"],
            networkPrefixLengths: [128]
        )
        ipv6.includedRoutes = [NEIPv6Route.default()]
        settings.ipv6Settings = ipv6

        let servers = session.dns.isEmpty ? ["1.1.1.1", "8.8.8.8"] : session.dns
        let dns = NEDNSSettings(servers: servers)
        dns.matchDomains = [""]
        settings.dnsSettings = dns
        settings.mtu = 1500
        return settings
    }

    /// `getaddrinfo` has no timeout of its own, and a captive portal can hold it
    /// long enough for the system to tear the starting extension down. Resolve
    /// off the caller's queue and go on with whatever answered in time.
    private func resolve(_ hosts: [String]) -> [String] {
        var found = Set<String>()
        let guardLock = NSLock()
        let group = DispatchGroup()
        for host in hosts {
            group.enter()
            DispatchQueue.global(qos: .userInitiated).async {
                let addresses = lookupAddresses(host)
                guardLock.lock()
                found.formUnion(addresses)
                guardLock.unlock()
                group.leave()
            }
        }
        _ = group.wait(timeout: .now() + resolveTimeoutSeconds)
        guardLock.lock()
        defer { guardLock.unlock() }
        return Array(found)
    }

    private func writeReady(ok: Bool, footprint: UInt64 = physicalFootprint(), error: String) {
        let ready = ReadyFile(ok: ok, footprint: footprint, error: error)
        if let data = try? JSONEncoder().encode(ready) {
            try? AppGroup.writeAtomic("ready.json", data: data)
        }
    }

    private func clearReady() {
        if let url = AppGroup.file("ready.json") {
            try? FileManager.default.removeItem(at: url)
        }
    }

    private func tunnelError(_ message: String) -> NSError {
        NSError(
            domain: "io.github.xraybox.tunnel",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: message]
        )
    }
}

private let sessionWaitSeconds: TimeInterval = 45
private let resolveTimeoutSeconds: TimeInterval = 5

/*
/// The watchdog ticks every 2 seconds; resources are logged every 10.
private let resourceLogEveryTicks = 5

/// Cumulative process counters. Deltas between two samples give CPU share,
/// wakeups per second, and how much of the CPU the Go collector took.
private struct ResourceSample {
    let time: TimeInterval
    let cpuNanoseconds: UInt64
    let wakeups: UInt64
    let gcCycles: UInt64
    let gcCPUMillis: UInt64

    static func now() -> ResourceSample {
        let power = taskPowerInfo()
        return ResourceSample(
            time: ProcessInfo.processInfo.systemUptime,
            cpuNanoseconds: processCPUNanoseconds(),
            wakeups: power.map { $0.task_interrupt_wakeups + $0.task_platform_idle_wakeups } ?? 0,
            gcCycles: UInt64(XrayGCCycles()),
            gcCPUMillis: UInt64(XrayGCCPUMillis())
        )
    }

    func describe(since last: ResourceSample) -> String? {
        let seconds = time - last.time
        guard seconds > 0, cpuNanoseconds >= last.cpuNanoseconds else { return nil }
        let cpuMillis = Double(cpuNanoseconds - last.cpuNanoseconds) / 1_000_000
        let gcMillis = Double(gcCPUMillis &- last.gcCPUMillis)
        let cpuPercent = cpuMillis / (seconds * 10)
        let gcShare = cpuMillis > 0 ? gcMillis / cpuMillis * 100 : 0
        let wakesPerSecond = Double(wakeups &- last.wakeups) / seconds
        let gcPerSecond = Double(gcCycles &- last.gcCycles) / seconds
        return String(
            format: "cpu=%.1f%% wakes=%.0f/s gc=%.1f/s gcShare=%.0f%%",
            cpuPercent,
            wakesPerSecond,
            gcPerSecond,
            gcShare
        )
    }
}

private func processCPUNanoseconds() -> UInt64 {
    var usage = rusage()
    if getrusage(RUSAGE_SELF, &usage) != 0 {
        return 0
    }
    func nanoseconds(_ value: timeval) -> UInt64 {
        UInt64(value.tv_sec) * 1_000_000_000 + UInt64(value.tv_usec) * 1_000
    }
    return nanoseconds(usage.ru_utime) + nanoseconds(usage.ru_stime)
}

private func taskPowerInfo() -> task_power_info? {
    var info = task_power_info()
    var count = mach_msg_type_number_t(
        MemoryLayout<task_power_info>.stride / MemoryLayout<integer_t>.stride
    )
    let result = withUnsafeMutablePointer(to: &info) { pointer -> kern_return_t in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { rebound in
            task_info(mach_task_self_, task_flavor_t(TASK_POWER_INFO), rebound, &count)
        }
    }
    return result == KERN_SUCCESS ? info : nil
}
*/

private func lookupAddresses(_ host: String) -> [String] {
    var info: UnsafeMutablePointer<addrinfo>?
    let hints = UnsafeMutablePointer<addrinfo>.allocate(capacity: 1)
    defer { hints.deallocate() }
    hints.initialize(to: addrinfo())
    hints.pointee.ai_family = AF_UNSPEC
    hints.pointee.ai_socktype = SOCK_STREAM
    if getaddrinfo(host, nil, hints, &info) != 0 {
        return []
    }
    defer { freeaddrinfo(info) }
    var addresses = [String]()
    var cursor = info
    while let node = cursor {
        if let address = ipString(node.pointee.ai_addr) {
            addresses.append(address)
        }
        cursor = node.pointee.ai_next
    }
    return addresses
}

private func ipString(_ address: UnsafePointer<sockaddr>?) -> String? {
    guard let address else { return nil }
    var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
    let length = socklen_t(address.pointee.sa_len)
    let result = getnameinfo(address, length, &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST)
    if result != 0 {
        return nil
    }
    let text = String(cString: host)
    return text.isEmpty ? nil : text
}

private func physicalFootprint() -> UInt64 {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(
        MemoryLayout<task_vm_info_data_t>.stride / MemoryLayout<integer_t>.stride
    )
    let result = withUnsafeMutablePointer(to: &info) { pointer -> kern_return_t in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { rebound in
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), rebound, &count)
        }
    }
    if result == KERN_SUCCESS {
        return UInt64(info.phys_footprint)
    }
    return 0
}
