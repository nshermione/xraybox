package io.github.xraybox.vpn

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import io.github.xraybox.BootLog
import io.github.xraybox.MainActivity
import io.github.xraybox.R

class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    val action = intent?.action.orEmpty()
    if (action !in HANDLED_ACTIONS) {
      return
    }
    BootLog.init(context, dumpPrevious = false)
    val settings = EngineSettingsStore.load(context)
    val settingsPath = EngineSettingsStore.foundSettingsFile(context)?.absolutePath ?: "none"
    val hasDesired = EngineSettingsStore.hasFile(context, EngineSettingsStore.DESIRED_FILE)
    val hasLast = EngineSettingsStore.hasFile(context, EngineSettingsStore.LAST_FILE)
    BootLog.line(
      "boot action=$action settings=$settingsPath startOnBoot=${settings.startOnBoot} " +
        "auto=${settings.autoConnect} desired=$hasDesired last=$hasLast",
    )
    if (!EngineSettingsStore.shouldRestoreOnBoot(settings, hasDesired, hasLast)) {
      BootLog.line("boot skip restore")
      return
    }
    if (VpnService.prepare(context) != null) {
      BootLog.line("boot vpn consent missing")
      notifyOpenApp(context)
      return
    }
    try {
      ContextCompat.startForegroundService(
        context,
        Intent(context, XrayVpnService::class.java).setAction(XrayVpnService.ACTION_RESTORE),
      )
      BootLog.line("boot started vpn service")
    } catch (err: Exception) {
      BootLog.line("boot startForegroundService failed: ${err.message}")
    }
  }

  private fun notifyOpenApp(context: Context) {
    if (
      Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS)
        != PackageManager.PERMISSION_GRANTED
    ) {
      BootLog.line("boot notification permission missing")
      return
    }
    ensureChannel(context)
    val launch = PendingIntent.getActivity(
      context,
      0,
      Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val notification = NotificationCompat.Builder(context, CHANNEL_ID)
      .setSmallIcon(R.drawable.ic_vpn_notification)
      .setContentTitle(context.getString(R.string.vpn_boot_title))
      .setContentText(context.getString(R.string.vpn_boot_text))
      .setContentIntent(launch)
      .setAutoCancel(true)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .build()
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    manager.notify(NOTIFICATION_ID, notification)
  }

  private fun ensureChannel(context: Context) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
      return
    }
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    manager.createNotificationChannel(
      NotificationChannel(
        CHANNEL_ID,
        context.getString(R.string.vpn_boot_channel),
        NotificationManager.IMPORTANCE_DEFAULT,
      ),
    )
  }

  companion object {
    private val HANDLED_ACTIONS = setOf(
      Intent.ACTION_BOOT_COMPLETED,
      Intent.ACTION_MY_PACKAGE_REPLACED,
      "android.intent.action.QUICKBOOT_POWERON",
      "com.htc.intent.action.QUICKBOOT_POWERON",
    )
    private const val CHANNEL_ID = "xraybox.boot"
    private const val NOTIFICATION_ID = 20
  }
}
