package io.github.xraybox

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object BootLog {
  const val TAG = "xraybox-boot"
  const val FILE_NAME = "xraybox-boot.log"
  const val FILE_PATH = "/data/data/io.github.xraybox/files/$FILE_NAME"

  private const val MAX_BYTES = 256 * 1024
  private val time = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)

  @Volatile
  private var file: File? = null

  @Volatile
  private var sessionStarted = false

  @Synchronized
  fun init(context: Context, dumpPrevious: Boolean) {
    val target = File(context.filesDir, FILE_NAME)
    rotateIfNeeded(target)
    file = target
    if (dumpPrevious) {
      dumpToLogcat(target)
    }
    if (sessionStarted) return
    sessionStarted = true
    line("---- session pid=${android.os.Process.myPid()} file=${target.absolutePath} ----")
  }

  fun line(message: String) {
    val stamped = "${time.format(Date())} $message"
    Log.i(TAG, message)
    val target = file ?: File(FILE_PATH)
    try {
      FileOutputStream(target, true).use { out ->
        out.write((stamped + "\n").toByteArray(Charsets.UTF_8))
        out.flush()
        out.fd.sync()
      }
    } catch (err: Exception) {
      Log.w(TAG, "write failed", err)
    }
  }

  fun sync() {
    val target = file ?: return
    try {
      FileOutputStream(target, true).use { it.fd.sync() }
    } catch (_: Exception) {
    }
  }

  private fun dumpToLogcat(target: File) {
    if (!target.isFile || target.length() == 0L) {
      Log.i(TAG, "no previous boot log at ${target.absolutePath}")
      return
    }
    Log.i(TAG, "==== previous ${target.absolutePath} (${target.length()}b) ====")
    try {
      target.forEachLine { Log.i(TAG, it) }
    } catch (err: Exception) {
      Log.w(TAG, "dump failed", err)
    }
    Log.i(TAG, "==== end previous boot log ====")
  }

  private fun rotateIfNeeded(target: File) {
    if (!target.isFile || target.length() < MAX_BYTES) return
    val keep = target.readBytes().let { bytes ->
      bytes.copyOfRange(bytes.size - MAX_BYTES / 4, bytes.size)
    }
    target.writeBytes(keep)
  }
}
