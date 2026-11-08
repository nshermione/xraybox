package io.github.xraybox.vpn

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.ResultReceiver
import android.os.SystemClock
import android.provider.Settings
import androidx.activity.result.ActivityResult
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@InvokeArg
class EstablishArgs {
  var dns: List<String> = emptyList()
  var excludeHosts: List<String> = emptyList()
  var configPath: String = ""
  var workdir: String = ""
  var assetDir: String = ""
  var sidecarPath: String = ""
  var killSwitch: Boolean = false
}

@InvokeArg
class FlagArgs {
  var enabled: Boolean = false
}

data class Established(
  val sidecarPath: String,
)

data class SidecarPath(
  val path: String,
)

data class Running(
  val running: Boolean,
)

@TauriPlugin(
  permissions = [
    Permission(
      strings = [Manifest.permission.POST_NOTIFICATIONS],
      alias = "notifications",
    ),
  ],
)
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun prepare(invoke: Invoke) {
    if (
      Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      ActivityCompat.checkSelfPermission(
        activity,
        Manifest.permission.POST_NOTIFICATIONS,
      ) != PackageManager.PERMISSION_GRANTED
    ) {
      requestPermissionForAlias("notifications", invoke, "afterNotifications")
      return
    }
    requestVpnConsent(invoke)
  }

  @PermissionCallback
  fun afterNotifications(invoke: Invoke) {
    requestVpnConsent(invoke)
  }

  @ActivityCallback
  fun vpnConsentResult(invoke: Invoke, result: ActivityResult) {
    if (result.resultCode == Activity.RESULT_OK) {
      invoke.resolve()
    } else {
      invoke.reject("vpn_denied", "vpn_denied")
    }
  }

  @Command
  fun establish(invoke: Invoke) {
    val args = invoke.parseArgs(EstablishArgs::class.java)
    val sidecar = sidecarFile()
    if (!sidecar.isFile) {
      invoke.reject("xray_sidecar_missing", "xray_sidecar_missing")
      return
    }
    val sidecarPath = args.sidecarPath.ifBlank { sidecar.absolutePath }

    Thread {
      val (code, error) = sendService(Intent(activity, XrayVpnService::class.java).apply {
        putStringArrayListExtra(XrayVpnService.EXTRA_DNS, ArrayList(args.dns))
        putStringArrayListExtra(XrayVpnService.EXTRA_EXCLUDE, ArrayList(args.excludeHosts))
        putExtra(XrayVpnService.EXTRA_CONFIG, args.configPath)
        putExtra(XrayVpnService.EXTRA_WORKDIR, args.workdir)
        putExtra(XrayVpnService.EXTRA_ASSET_DIR, args.assetDir)
        putExtra(XrayVpnService.EXTRA_SIDECAR, sidecarPath)
        putExtra(XrayVpnService.EXTRA_KILL_SWITCH, args.killSwitch)
      }, 20_000)
      if (code == XrayVpnService.RESULT_OK) {
        invoke.resolveObject(Established(sidecarPath))
      } else {
        invoke.reject(error ?: "vpn_establish_failed")
      }
    }.start()
  }

  @Command
  fun teardown(invoke: Invoke) {
    Thread {
      sendService(
        Intent(activity, XrayVpnService::class.java).apply {
          action = XrayVpnService.ACTION_TEARDOWN
        },
        8_000,
      )
      activity.stopService(Intent(activity, XrayVpnService::class.java))
      invoke.resolve()
    }.start()
  }

  @Command
  fun running(invoke: Invoke) {
    io.github.xraybox.BootLog.line("plugin running() queued")
    Thread {
      val started = SystemClock.elapsedRealtime()
      if (!vpnProcessRunning()) {
        io.github.xraybox.BootLog.line("plugin running() vpn process down")
        invoke.resolveObject(Running(false))
        return@Thread
      }
      io.github.xraybox.BootLog.line("plugin running() sendService")
      val (code, _) = sendService(
        Intent(activity, XrayVpnService::class.java).apply {
          action = XrayVpnService.ACTION_STATUS
        },
        4_000,
      )
      io.github.xraybox.BootLog.line("plugin running() done code=$code ${SystemClock.elapsedRealtime() - started}ms")
      invoke.resolveObject(Running(code == XrayVpnService.RESULT_OK))
    }.start()
  }

  @Command
  fun applyStartOnBoot(invoke: Invoke) {
    val args = invoke.parseArgs(FlagArgs::class.java)
    if (args.enabled) {
      activity.runOnUiThread { requestBatteryExemption() }
    }
    invoke.resolve()
  }

  @Command
  fun setKillSwitch(invoke: Invoke) {
    val args = invoke.parseArgs(FlagArgs::class.java)
    Thread {
      sendService(
        Intent(activity, XrayVpnService::class.java).apply {
          action = XrayVpnService.ACTION_KILL_SWITCH
          putExtra(XrayVpnService.EXTRA_KILL_SWITCH, args.enabled)
        },
        4_000,
      )
      invoke.resolve()
    }.start()
  }

  @Command
  fun sidecarPath(invoke: Invoke) {
    val sidecar = sidecarFile()
    if (!sidecar.isFile) {
      invoke.reject("xray_sidecar_missing", "xray_sidecar_missing")
      return
    }
    invoke.resolveObject(SidecarPath(sidecar.absolutePath))
  }

  private fun requestVpnConsent(invoke: Invoke) {
    val consent = VpnService.prepare(activity)
    if (consent != null) {
      startActivityForResult(invoke, consent, "vpnConsentResult")
    } else {
      invoke.resolve()
    }
  }

  private fun requestBatteryExemption() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
      return
    }
    val pm = activity.getSystemService(PowerManager::class.java) ?: return
    if (pm.isIgnoringBatteryOptimizations(activity.packageName)) {
      return
    }
    try {
      activity.startActivity(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
          data = Uri.parse("package:${activity.packageName}")
        },
      )
    } catch (_: Exception) {
      try {
        activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
      } catch (_: Exception) {
      }
    }
  }

  private fun sidecarFile(): File {
    return File(activity.applicationInfo.nativeLibraryDir, XrayVpnService.SIDECAR_NAME)
  }

  private fun vpnProcessRunning(): Boolean {
    val manager = activity.getSystemService(ActivityManager::class.java) ?: return false
    val name = "${activity.packageName}:vpn"
    return manager.runningAppProcesses.orEmpty().any { it.processName == name }
  }

  private fun sendService(intent: Intent, timeoutMs: Long): Pair<Int, String?> {
    val latch = CountDownLatch(1)
    var code = XrayVpnService.RESULT_ERR
    var error: String? = "vpn establish timeout"
    val receiver = object : ResultReceiver(Handler(Looper.getMainLooper())) {
      override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
        code = resultCode
        error = resultData?.getString(XrayVpnService.EXTRA_ERROR)
        latch.countDown()
      }
    }
    intent.putExtra(XrayVpnService.EXTRA_RECEIVER, receiver)
    val action = intent.action
    io.github.xraybox.BootLog.line("sendService action=$action timeout=${timeoutMs}ms")
    if (
      action == XrayVpnService.ACTION_STATUS ||
      action == XrayVpnService.ACTION_TEARDOWN ||
      action == XrayVpnService.ACTION_KILL_SWITCH
    ) {
      activity.startService(intent)
    } else {
      ContextCompat.startForegroundService(activity, intent)
    }
    if (!latch.await(timeoutMs, TimeUnit.MILLISECONDS)) {
      io.github.xraybox.BootLog.line("sendService timeout action=$action")
      return XrayVpnService.RESULT_ERR to "vpn establish timeout"
    }
    io.github.xraybox.BootLog.line("sendService replied action=$action code=$code")
    return code to error
  }
}
