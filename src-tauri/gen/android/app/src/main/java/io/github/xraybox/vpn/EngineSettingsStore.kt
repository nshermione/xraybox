package io.github.xraybox.vpn

import android.content.Context
import java.io.File
import org.json.JSONObject

object EngineSettingsStore {
  // Rust writes engine.json under Tauri app_data_dir(), which on Android is
  // context.dataDir (/data/user/0/<id>). vpn-desired.json / vpn-last.json
  // stay under filesDir because XrayVpnService owns them.
  const val RELATIVE = "settings/engine.json"
  const val DESIRED_FILE = "vpn-desired.json"
  const val LAST_FILE = "vpn-last.json"

  data class Settings(
    val autoConnect: Boolean = false,
    val startOnBoot: Boolean = false,
    val killSwitch: Boolean = false,
  )

  fun settingsFile(dataDir: File): File = File(dataDir, RELATIVE)

  fun settingsCandidates(context: Context): List<File> {
    return listOf(settingsFile(context.dataDir), settingsFile(context.filesDir))
  }

  fun foundSettingsFile(context: Context): File? {
    return settingsCandidates(context).firstOrNull { it.isFile }
  }

  fun load(context: Context): Settings {
    return loadFromCandidates(settingsCandidates(context))
  }

  fun loadFromCandidates(candidates: List<File>): Settings {
    for (file in candidates) {
      if (!file.isFile) {
        continue
      }
      try {
        return parse(file.readText())
      } catch (_: Exception) {
      }
    }
    return Settings()
  }

  fun parse(json: String): Settings {
    val obj = JSONObject(json)
    return Settings(
      autoConnect = obj.optBoolean("autoConnect", false),
      startOnBoot = obj.optBoolean("startOnBoot", false),
      killSwitch = obj.optBoolean(
        "killSwitch",
        obj.optBoolean("persistTunnelOnServerChange", false),
      ),
    )
  }

  fun shouldRestoreOnBoot(
    settings: Settings,
    hasDesired: Boolean,
    hasLast: Boolean,
  ): Boolean {
    return settings.startOnBoot && (hasDesired || (settings.autoConnect && hasLast))
  }

  fun shouldHoldTun(killSwitch: Boolean, userRequestedStop: Boolean): Boolean {
    return killSwitch && !userRequestedStop
  }

  fun resolveKillSwitch(
    fromIntent: Boolean?,
    fromSettings: Boolean?,
    fromRequest: Boolean?,
  ): Boolean {
    return fromIntent ?: fromSettings ?: fromRequest ?: false
  }

  fun resolveSidecar(stored: File, fallback: File): File {
    return if (stored.isFile) stored else fallback
  }

  fun hasFile(context: Context, name: String): Boolean {
    return File(context.filesDir, name).isFile
  }
}
