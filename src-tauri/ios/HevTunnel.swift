import Foundation

/// Runs hev-socks5-tunnel on the utun descriptor Network Extension already
/// opened. Packets never pass through Swift. The descriptor belongs to the
/// system, so `stop` does not close it.
final class HevTunnel {
    private let state = NSLock()
    private var running = false
    private var bridgeFailed = false
    private let hevExited = DispatchSemaphore(value: 0)

    init(tunFd: Int32, socksPort: UInt16) {
        running = true
        let yaml = hevConfig(socksPort: socksPort)
        Thread.detachNewThread { [weak self] in
            let result = yaml.withCString { pointer -> Int32 in
                let bytes = UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self)
                return hev_socks5_tunnel_main_from_str(bytes, UInt32(strlen(pointer)), tunFd)
            }
            if result != 0 {
                NSLog("xraybox hev-socks5-tunnel exited \(result)")
            }
            guard let self else { return }
            self.hevExited.signal()
            if self.isRunning() {
                self.markFailed()
            }
        }
    }

    /// False once hev has gone. The provider watchdog rebuilds it instead of
    /// leaving a connected tunnel that carries nothing.
    func isHealthy() -> Bool {
        state.lock()
        defer { state.unlock() }
        return !bridgeFailed
    }

    func stop() {
        state.lock()
        let wasRunning = running
        running = false
        state.unlock()
        guard wasRunning else { return }

        hev_socks5_tunnel_quit()
        _ = hevExited.wait(timeout: .now() + 1)
    }

    private func isRunning() -> Bool {
        state.lock()
        defer { state.unlock() }
        return running
    }

    private func markFailed() {
        state.lock()
        bridgeFailed = true
        state.unlock()
    }
}

/// Every session owns a `task-stack-size` stack inside the 50MB extension
/// budget. Past `max-session-count` hev closes the least recently used one.
private func hevConfig(socksPort: UInt16) -> String {
    """
    tunnel:
      mtu: 1500
      ipv4: 172.19.0.1
    socks5:
      port: \(socksPort)
      address: 127.0.0.1
      udp: udp
    misc:
      task-stack-size: 24576
      tcp-buffer-size: 4096
      max-session-count: 256
      log-level: warn
    """
}
