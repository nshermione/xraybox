package io.github.xraybox.vpn

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EngineSettingsStoreTest {
  @Test
  fun parseReadsCamelCaseFlags() {
    val settings = EngineSettingsStore.parse(
      """{"autoConnect":true,"startOnBoot":true,"killSwitch":true}""",
    )
    assertTrue(settings.autoConnect)
    assertTrue(settings.startOnBoot)
    assertTrue(settings.killSwitch)
  }

  @Test
  fun parseReadsLegacyPersistTunnelFlag() {
    val settings = EngineSettingsStore.parse("""{"persistTunnelOnServerChange":true}""")
    assertTrue(settings.killSwitch)
    assertFalse(settings.autoConnect)
    assertFalse(settings.startOnBoot)
  }

  @Test
  fun restoreOnBootNeedsStartOnBoot() {
    val off = EngineSettingsStore.Settings(autoConnect = true, startOnBoot = false)
    val bootOnly = EngineSettingsStore.Settings(startOnBoot = true)
    val bootAuto = EngineSettingsStore.Settings(autoConnect = true, startOnBoot = true)
    assertFalse(EngineSettingsStore.shouldRestoreOnBoot(off, true, true))
    assertTrue(EngineSettingsStore.shouldRestoreOnBoot(bootOnly, true, false))
    assertTrue(EngineSettingsStore.shouldRestoreOnBoot(bootAuto, false, true))
    assertFalse(EngineSettingsStore.shouldRestoreOnBoot(bootAuto, false, false))
    assertFalse(EngineSettingsStore.shouldRestoreOnBoot(bootOnly, false, true))
  }

  @Test
  fun loadFromCandidatesPrefersFirstReadableFile() {
    val first = File.createTempFile("xraybox-engine-first", ".json")
    val second = File.createTempFile("xraybox-engine-second", ".json")
    try {
      first.writeText("""{"startOnBoot":true}""")
      second.writeText("""{"autoConnect":true}""")
      val loaded = EngineSettingsStore.loadFromCandidates(listOf(first, second))
      assertTrue(loaded.startOnBoot)
      assertFalse(loaded.autoConnect)
      assertTrue(first.delete())
      val fallback = EngineSettingsStore.loadFromCandidates(listOf(first, second))
      assertTrue(fallback.autoConnect)
      assertFalse(fallback.startOnBoot)
    } finally {
      first.delete()
      second.delete()
    }
  }

  @Test
  fun settingsFileLivesUnderDataDirNotFiles() {
    val path = EngineSettingsStore.settingsFile(File("/data/user/0/app")).path.replace('\\', '/')
    assertTrue(path.endsWith("settings/engine.json"))
    assertFalse(path.contains("/files/"))
  }

  @Test
  fun killSwitchPrefersIntentThenSettingsThenRequest() {
    assertTrue(EngineSettingsStore.resolveKillSwitch(true, false, false))
    assertFalse(EngineSettingsStore.resolveKillSwitch(false, true, true))
    assertTrue(EngineSettingsStore.resolveKillSwitch(null, true, false))
    assertFalse(EngineSettingsStore.resolveKillSwitch(null, false, true))
    assertTrue(EngineSettingsStore.resolveKillSwitch(null, null, true))
    assertFalse(EngineSettingsStore.resolveKillSwitch(null, null, null))
  }

  @Test
  fun sidecarFallsBackWhenStoredPathIsGone() {
    val stored = File.createTempFile("xraybox-sidecar-stored", ".so")
    val fallback = File.createTempFile("xraybox-sidecar-fallback", ".so")
    try {
      assertEquals(stored, EngineSettingsStore.resolveSidecar(stored, fallback))
      assertTrue(stored.delete())
      assertEquals(fallback, EngineSettingsStore.resolveSidecar(stored, fallback))
    } finally {
      stored.delete()
      fallback.delete()
    }
  }

  @Test
  fun holdTunOnlyWithKillSwitchUntilUserStops() {
    assertTrue(EngineSettingsStore.shouldHoldTun(true, false))
    assertFalse(EngineSettingsStore.shouldHoldTun(true, true))
    assertFalse(EngineSettingsStore.shouldHoldTun(false, false))
    assertEquals(false, EngineSettingsStore.shouldHoldTun(false, true))
  }
}
