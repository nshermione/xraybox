package io.github.xraybox

import android.os.Bundle
import android.os.Process
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import io.github.xraybox.vpn.XrayVpnService
import java.io.File
import java.net.URI
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    BootLog.init(this, dumpPrevious = true)
    BootLog.line("onCreate pid=${Process.myPid()} state=${savedInstanceState != null}")
    enableEdgeToEdge()
    // Fresh activity id every process. Restoring the previous Tauri id
    // after a kill leaves a window with no WebView.
    super.onCreate(null)
    BootLog.line("onCreate super done id=$id")
    hideNavigationBar()
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    BootLog.line("webview created url=${webView.url}")
    persistFrontendHost(webView.url)
    webView.postDelayed({
      BootLog.line("webview +3s url=${webView.url} progress=${webView.progress}")
      persistFrontendHost(webView.url)
    }, 3000)
  }

  private fun persistFrontendHost(url: String?) {
    val host = url?.let { runCatching { URI(it).host }.getOrNull() }?.trim().orEmpty()
    if (host.isEmpty() || host == "localhost" || host.endsWith(".localhost")) {
      return
    }
    val file = File(filesDir, XrayVpnService.FRONTEND_HOST_FILE)
    val existing = if (file.isFile) file.readLines().map { it.trim() } else emptyList()
    if (existing.contains(host)) {
      return
    }
    file.writeText((existing + host).filter { it.isNotEmpty() }.joinToString("\n"))
    BootLog.line("persist frontend host=$host")
  }

  override fun onDestroy() {
    BootLog.line("onDestroy pid=${Process.myPid()} recreating=$isChangingConfigurations")
    if (!isChangingConfigurations) {
      // SIGKILL: no Rust/C++ destructors, so no
      // "pthread_mutex_lock on a destroyed mutex". :vpn is a
      // separate process and keeps the tunnel.
      BootLog.line("SIGKILL UI process")
      BootLog.sync()
      Process.killProcess(Process.myPid())
      return
    }
    super.onDestroy()
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus) hideNavigationBar()
  }

  private fun hideNavigationBar() {
    val controller = WindowCompat.getInsetsController(window, window.decorView)
    controller.hide(WindowInsetsCompat.Type.navigationBars())
    controller.systemBarsBehavior =
      WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
  }
}
