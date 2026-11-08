package io.github.xraybox.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.IpPrefix
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.ParcelFileDescriptor
import android.os.ResultReceiver
import android.util.Log
import androidx.core.app.NotificationCompat
import io.github.xraybox.MainActivity
import io.github.xraybox.R
import java.io.File
import java.io.IOException
import java.net.Inet4Address
import java.net.InetAddress
import org.json.JSONArray
import org.json.JSONObject

class XrayVpnService : VpnService() {
  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_TEARDOWN) {
      reply(intent, RESULT_OK)
      shutdown(removeNotification = true, clearDesiredState = true)
      return START_NOT_STICKY
    }
    if (intent?.action == ACTION_KILL_SWITCH) {
      val previous = killSwitch
      killSwitch = intent.getBooleanExtra(EXTRA_KILL_SWITCH, false)
      io.github.xraybox.BootLog.init(this, dumpPrevious = false)
      io.github.xraybox.BootLog.line("service KILL_SWITCH enabled=$killSwitch tun=${heldPfd != null}")
      val held = currentRequest
      if (held != null && heldPfd != null && previous != killSwitch) {
        try {
          startTunnel(held.copy(killSwitch = killSwitch), reuseTun = false)
        } catch (err: Exception) {
          Log.w(TAG, "kill switch rebind", err)
        }
      }
      reply(intent, RESULT_OK)
      if (!isServiceNeeded()) {
        stopSelf(startId)
        return START_NOT_STICKY
      }
      return START_STICKY
    }
    if (intent?.action == ACTION_STATUS) {
      val up = isTunnelUp()
      io.github.xraybox.BootLog.init(this, dumpPrevious = false)
      io.github.xraybox.BootLog.line("service STATUS tunnelUp=$up hold=${shouldHoldTun()} xray=${isXrayAlive()}")
      reply(intent, if (up) RESULT_OK else RESULT_DOWN)
      if (!isServiceNeeded()) {
        stopSelf(startId)
        return START_NOT_STICKY
      }
      return START_STICKY
    }

    startAsForeground()
    val restore = intent == null
      || intent.action == ACTION_RESTORE
      || (intent.action == null && !intent.hasExtra(EXTRA_CONFIG))
    val request = if (restore) readDesired() ?: readLast() else readRequest(intent)
    if (request == null) {
      io.github.xraybox.BootLog.init(this, dumpPrevious = false)
      io.github.xraybox.BootLog.line("service restore=$restore has no stored request")
      reply(intent, RESULT_ERR, "vpn_establish_failed")
      if (!isServiceNeeded()) {
        shutdown(removeNotification = true, clearDesiredState = false)
      }
      return START_NOT_STICKY
    }
    refreshKillSwitch(intent, request)

    try {
      val resolved = startTunnel(request.copy(killSwitch = killSwitch))
      persistRequest(request.copy(excludeIps = resolved, killSwitch = killSwitch))
      writeRuntime(up = true)
      ensureWatchdog()
      io.github.xraybox.BootLog.init(this, dumpPrevious = false)
      io.github.xraybox.BootLog.line("persist exclude=${request.exclude} ips=$resolved killSwitch=$killSwitch")
      reply(intent, RESULT_OK)
    } catch (err: Exception) {
      Log.e(TAG, "establish failed", err)
      io.github.xraybox.BootLog.init(this, dumpPrevious = false)
      io.github.xraybox.BootLog.line("service establish failed restore=$restore: ${err.message}")
      reply(intent, RESULT_ERR, err.message ?: "vpn_establish_failed")
      if (!shouldHoldTun()) {
        shutdown(removeNotification = true, clearDesiredState = false)
        return START_NOT_STICKY
      }
      ensureWatchdog()
    }
    return START_STICKY
  }

  // A revoke is the system taking the tunnel away (another VPN, or the device
  // shutting down). It is not the user pressing disconnect, so the desired
  // state has to survive it or start-on-boot has nothing left to restore.
  override fun onRevoke() {
    shutdown(removeNotification = true, clearDesiredState = false)
  }

  override fun onDestroy() {
    stopWatchdog()
    killXray()
    closeTunnel()
    writeRuntime(up = false)
    super.onDestroy()
  }

  private fun startTunnel(request: StartRequest, reuseTun: Boolean = false): List<String> {
    currentRequest = request
    val (fd, resolved) = if (reuseTun && heldPfd != null) {
      heldPfd!!.fd to request.excludeIps
    } else {
      try {
        establishTunnel(request.dns, request.exclude, request.excludeIps, request.killSwitch)
      } catch (err: Exception) {
        if (!request.killSwitch) {
          throw err
        }
        Log.w(TAG, "ipv6 tun failed, retry ipv4-only kill switch", err)
        establishTunnel(request.dns, request.exclude, request.excludeIps, false)
      }
    }
    killXray()
    val pid = spawnXray(request, fd)
    if (pid <= 0) {
      throw IllegalStateException("failed to spawn xray")
    }
    xrayPid = pid
    Log.i(TAG, "xray pid=$pid tun=$fd excludeIps=$resolved killSwitch=${request.killSwitch}")
    return resolved
  }

  private fun establishTunnel(
    dnsServers: List<String>,
    excludeHosts: List<String>,
    persistedIps: List<String>,
    blockIpv6: Boolean,
  ): Pair<Int, List<String>> {
    val builder = Builder()
      .setSession("xraybox")
      .setMtu(TUN_MTU)
      .addAddress(TUN_ADDRESS, TUN_PREFIX)
      .addRoute("0.0.0.0", 0)
      .setBlocking(false)
    if (blockIpv6) {
      try {
        builder.addAddress(TUN_IPV6_ADDRESS, TUN_IPV6_PREFIX)
        builder.addRoute("::", 0)
      } catch (err: Exception) {
        Log.w(TAG, "ipv6 kill switch route", err)
      }
    }

    val servers = dnsServers.ifEmpty { listOf("1.1.1.1", "8.8.8.8") }
    for (server in servers) {
      builder.addDnsServer(server)
    }
    val resolved = excludeFromTunnel(
      builder,
      excludeHosts + persistedFrontendHosts(),
      persistedIps,
    )

    val pfd = builder.establish()
      ?: throw IllegalStateException("VpnService.Builder.establish returned null")
    val previous = heldPfd
    heldPfd = pfd
    if (previous != null && previous !== pfd) {
      try {
        previous.close()
      } catch (err: IOException) {
        Log.w(TAG, "close replaced tun fd", err)
      }
    }
    return pfd.fd to resolved
  }

  private fun spawnXray(request: StartRequest, tunFd: Int): Int {
    val sidecar = resolveSidecarFile(request.sidecar)
    if (!sidecar.isFile) {
      throw IllegalStateException("xray_sidecar_missing")
    }
    File(request.workdir).mkdirs()
    val args = arrayOf(sidecar.absolutePath, "run", "-c", request.config)
    val env = System.getenv().map { "${it.key}=${it.value}" }.toMutableList()
    env.add("XRAY_LOCATION_ASSET=${request.assetDir}")
    env.add("XRAY_TUN_FD=$tunFd")
    env.add("xray.tun.fd=$tunFd")
    env.add("LD_LIBRARY_PATH=${sidecar.parent}")
    val log = File(request.workdir, "xray.log").absolutePath
    return nativeSpawnXray(
      sidecar.absolutePath,
      args,
      env.toTypedArray(),
      tunFd,
      request.workdir,
      log,
    )
  }

  private fun shutdown(removeNotification: Boolean, clearDesiredState: Boolean) {
    stopWatchdog()
    killXray()
    closeTunnel()
    currentRequest = null
    if (clearDesiredState) {
      clearDesired()
    }
    writeRuntime(up = false)
    if (removeNotification) {
      stopForeground(STOP_FOREGROUND_REMOVE)
    }
    stopSelf()
  }

  private fun persistRequest(request: StartRequest) {
    val sidecar = resolveSidecarFile(request.sidecar)
    val stored = request.copy(sidecar = sidecar.absolutePath)
    currentRequest = stored
    writeRequest(desiredFile(), stored)
    writeRequest(lastFile(), stored)
  }

  private fun refreshKillSwitch(intent: Intent?, request: StartRequest? = null) {
    val fromIntent = if (intent?.hasExtra(EXTRA_KILL_SWITCH) == true) {
      intent.getBooleanExtra(EXTRA_KILL_SWITCH, false)
    } else {
      null
    }
    val fromSettings = if (EngineSettingsStore.foundSettingsFile(this) != null) {
      EngineSettingsStore.load(this).killSwitch
    } else {
      null
    }
    killSwitch = EngineSettingsStore.resolveKillSwitch(
      fromIntent,
      fromSettings,
      request?.killSwitch,
    )
  }

  private fun resolveSidecarFile(stored: String): File {
    return EngineSettingsStore.resolveSidecar(
      File(stored),
      File(applicationInfo.nativeLibraryDir, SIDECAR_NAME),
    )
  }

  private fun shouldHoldTun(): Boolean {
    return EngineSettingsStore.shouldHoldTun(killSwitch, userRequestedStop = false) && heldPfd != null
  }

  private fun isServiceNeeded(): Boolean {
    return isTunnelUp() || shouldHoldTun()
  }

  private fun isXrayAlive(): Boolean {
    val pid = xrayPid
    return pid > 1 && File("/proc/$pid").exists()
  }

  private fun ensureWatchdog() {
    if (watchdogRunning) {
      return
    }
    watchdogRunning = true
    Thread({
      try {
        while (watchdogRunning) {
          Thread.sleep(WATCHDOG_MS)
          if (!watchdogRunning) {
            break
          }
          if (isXrayAlive()) {
            continue
          }
          if (!shouldHoldTun()) {
            continue
          }
          val request = currentRequest ?: readLast() ?: continue
          try {
            startTunnel(request.copy(killSwitch = true), reuseTun = true)
            writeRuntime(up = true)
            io.github.xraybox.BootLog.line("kill-switch respawned xray")
          } catch (err: Exception) {
            Log.e(TAG, "kill-switch respawn failed", err)
          }
        }
      } finally {
        watchdogRunning = false
      }
    }, "xray-watchdog").start()
  }

  private fun stopWatchdog() {
    watchdogRunning = false
  }

  private fun killXray() {
    val pid = xrayPid
    xrayPid = 0
    if (pid > 1) {
      nativeKillXray(pid)
    }
  }

  private fun closeTunnel() {
    val held = heldPfd
    heldPfd = null
    if (held != null) {
      try {
        held.close()
      } catch (err: IOException) {
        Log.w(TAG, "close tun fd", err)
      }
    }
  }

  private fun startAsForeground() {
    ensureChannel()
    val launch = PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val notification: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
      .setSmallIcon(R.drawable.ic_vpn_notification)
      .setContentTitle(getString(R.string.vpn_notification_title))
      .setContentText(getString(R.string.vpn_notification_text))
      .setContentIntent(launch)
      .setOngoing(true)
      .setCategory(NotificationCompat.CATEGORY_SERVICE)
      .build()

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      startForeground(
        NOTIFICATION_ID,
        notification,
        ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
      )
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
  }

  private fun excludeFromTunnel(
    builder: Builder,
    hosts: List<String>,
    persistedIps: List<String>,
  ): List<String> {
    val resolved = LinkedHashSet<String>()
    for (ip in persistedIps) {
      val trimmed = ip.trim()
      if (trimmed.isNotEmpty()) {
        resolved.add(trimmed)
      }
    }
    for (host in hosts) {
      for (address in resolveHost(host)) {
        address.hostAddress?.let { resolved.add(it) }
      }
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      for (ip in resolved) {
        try {
          val address = InetAddress.getByName(ip)
          val prefix = if (address is Inet4Address) 32 else 128
          builder.excludeRoute(IpPrefix(address, prefix))
          Log.i(TAG, "excludeRoute $ip/$prefix")
        } catch (err: Exception) {
          Log.w(TAG, "excludeRoute $ip", err)
        }
      }
    }
    return resolved.toList()
  }

  private fun persistedFrontendHosts(): List<String> {
    val file = File(filesDir, FRONTEND_HOST_FILE)
    if (!file.isFile) {
      return emptyList()
    }
    return file.readLines().map { it.trim() }.filter { it.isNotEmpty() }
  }

  private fun resolveHost(host: String): List<InetAddress> {
    val name = host.trim().removePrefix("[").removeSuffix("]")
    if (name.isEmpty()) {
      return emptyList()
    }
    return try {
      InetAddress.getAllByName(name).toList()
    } catch (err: Exception) {
      Log.w(TAG, "resolve $name", err)
      emptyList()
    }
  }

  private fun ensureChannel() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
      return
    }
    val manager = getSystemService(NotificationManager::class.java) ?: return
    val channel = NotificationChannel(
      CHANNEL_ID,
      getString(R.string.vpn_notification_channel),
      NotificationManager.IMPORTANCE_LOW,
    )
    channel.setShowBadge(false)
    manager.createNotificationChannel(channel)
  }

  private fun isTunnelUp(): Boolean {
    return (heldPfd != null && isXrayAlive()) || shouldHoldTun()
  }

  private fun reply(intent: Intent?, code: Int, error: String? = null) {
    val receiver = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      intent?.getParcelableExtra(EXTRA_RECEIVER, ResultReceiver::class.java)
    } else {
      @Suppress("DEPRECATION")
      intent?.getParcelableExtra(EXTRA_RECEIVER)
    }
    val data = Bundle()
    if (error != null) {
      data.putString(EXTRA_ERROR, error)
    }
    receiver?.send(code, data)
  }

  private fun desiredFile(): File = File(filesDir, DESIRED_FILE)

  private fun lastFile(): File = File(filesDir, LAST_FILE)

  private fun runtimeFile(): File = File(filesDir, RUNTIME_FILE)

  private fun writeRequest(file: File, request: StartRequest) {
    val json = JSONObject()
      .put("config", request.config)
      .put("workdir", request.workdir)
      .put("assetDir", request.assetDir)
      .put("sidecar", request.sidecar)
      .put("dns", JSONArray(request.dns))
      .put("exclude", JSONArray(request.exclude))
      .put("excludeIps", JSONArray(request.excludeIps))
      .put("killSwitch", request.killSwitch)
    file.writeText(json.toString())
  }

  private fun readDesired(): StartRequest? {
    val file = desiredFile()
    if (!file.isFile) {
      return null
    }
    return try {
      parseRequest(JSONObject(file.readText()))
    } catch (err: Exception) {
      Log.w(TAG, "read desired", err)
      null
    }
  }

  private fun clearDesired() {
    desiredFile().delete()
  }

  private fun writeRuntime(up: Boolean) {
    val json = JSONObject()
      .put("up", up)
      .put("pid", xrayPid)
    runtimeFile().writeText(json.toString())
  }

  private fun readRequest(intent: Intent?): StartRequest? {
    if (intent == null) {
      return null
    }
    val config = intent.getStringExtra(EXTRA_CONFIG) ?: return null
    return StartRequest(
      config = config,
      workdir = intent.getStringExtra(EXTRA_WORKDIR).orEmpty(),
      assetDir = intent.getStringExtra(EXTRA_ASSET_DIR).orEmpty(),
      sidecar = intent.getStringExtra(EXTRA_SIDECAR)
        ?: File(applicationInfo.nativeLibraryDir, SIDECAR_NAME).absolutePath,
      dns = intent.getStringArrayListExtra(EXTRA_DNS).orEmpty(),
      exclude = intent.getStringArrayListExtra(EXTRA_EXCLUDE).orEmpty(),
      excludeIps = emptyList(),
      killSwitch = intent.getBooleanExtra(EXTRA_KILL_SWITCH, killSwitch),
    )
  }

  private fun parseRequest(json: JSONObject): StartRequest {
    return StartRequest(
      config = json.getString("config"),
      workdir = json.getString("workdir"),
      assetDir = json.getString("assetDir"),
      sidecar = json.getString("sidecar"),
      dns = json.optJSONArray("dns").toStringList(),
      exclude = json.optJSONArray("exclude").toStringList(),
      excludeIps = json.optJSONArray("excludeIps").toStringList(),
      killSwitch = json.optBoolean("killSwitch", killSwitch),
    )
  }

  private fun readLast(): StartRequest? {
    val file = lastFile()
    if (!file.isFile) {
      return null
    }
    return try {
      parseRequest(JSONObject(file.readText()))
    } catch (err: Exception) {
      Log.w(TAG, "read last", err)
      null
    }
  }

  private fun JSONArray?.toStringList(): List<String> {
    if (this == null) {
      return emptyList()
    }
    return List(length()) { i -> optString(i) }.filter { it.isNotEmpty() }
  }

  private external fun nativeSpawnXray(
    path: String,
    args: Array<String>,
    envp: Array<String>,
    tunFd: Int,
    cwd: String,
    logPath: String,
  ): Int

  private external fun nativeKillXray(pid: Int)

  companion object {
    init {
      System.loadLibrary("xray_spawn")
    }

    const val TAG = "XrayVpnService"
    const val ACTION_TEARDOWN = "io.github.xraybox.vpn.TEARDOWN"
    const val ACTION_STATUS = "io.github.xraybox.vpn.STATUS"
    const val ACTION_RESTORE = "io.github.xraybox.vpn.RESTORE"
    const val ACTION_KILL_SWITCH = "io.github.xraybox.vpn.KILL_SWITCH"
    const val EXTRA_DNS = "dns"
    const val EXTRA_EXCLUDE = "exclude"
    const val EXTRA_CONFIG = "config"
    const val EXTRA_WORKDIR = "workdir"
    const val EXTRA_ASSET_DIR = "assetDir"
    const val EXTRA_SIDECAR = "sidecar"
    const val EXTRA_RECEIVER = "receiver"
    const val EXTRA_ERROR = "error"
    const val EXTRA_KILL_SWITCH = "killSwitch"
    const val SIDECAR_NAME = "libxray.so"
    const val TUN_MTU = 1500
    const val TUN_ADDRESS = "172.19.0.2"
    const val TUN_PREFIX = 30
    const val TUN_IPV6_ADDRESS = "fd00:19::2"
    const val TUN_IPV6_PREFIX = 126
    const val RESULT_OK = 0
    const val RESULT_DOWN = 1
    const val RESULT_ERR = 2
    private const val CHANNEL_ID = "xraybox.vpn"
    private const val NOTIFICATION_ID = 19
    const val DESIRED_FILE = "vpn-desired.json"
    const val LAST_FILE = "vpn-last.json"
    private const val RUNTIME_FILE = "vpn-runtime.json"
    const val FRONTEND_HOST_FILE = "vpn-frontend-host.txt"
    private const val WATCHDOG_MS = 2_000L

    @Volatile
    private var heldPfd: ParcelFileDescriptor? = null

    @Volatile
    private var xrayPid: Int = 0

    @Volatile
    private var killSwitch: Boolean = false

    @Volatile
    private var currentRequest: StartRequest? = null

    @Volatile
    private var watchdogRunning: Boolean = false
  }

  private data class StartRequest(
    val config: String,
    val workdir: String,
    val assetDir: String,
    val sidecar: String,
    val dns: List<String>,
    val exclude: List<String>,
    val excludeIps: List<String> = emptyList(),
    val killSwitch: Boolean = false,
  )
}
