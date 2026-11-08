<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useNavigation } from "../navigation/useNavigation";
import { t } from "../state/locale";
import { dnsEditorsLocked, dnsState, setFallbackToSystem, setRemoteServers } from "../state/dns";
import { loadManualEndpoint, saveManualEndpoint } from "../state/endpoint";
import { applyConnectedProfile } from "../state/vpn";
import Screen from "../ui/Screen.vue";
import List from "../ui/List.vue";
import ListItem from "../ui/ListItem.vue";
import Switch from "../ui/Switch.vue";
import TextField from "../ui/TextField.vue";
import Button from "../ui/Button.vue";
import HelpTip from "../ui/HelpTip.vue";
import { nestedItemsFor } from "./settingsData";

const props = withDefaults(
  defineProps<{
    titleKey: string;
    parentTitleKey?: string;
    screen?: "list" | "fields" | "vless" | "dns";
  }>(),
  {
    parentTitleKey: "nav.settings",
    screen: "list",
  },
);

const { push, back } = useNavigation();
const title = computed(() => t(props.titleKey));
const parentTitle = computed(() => t(props.parentTitleKey));
const items = nestedItemsFor(props.titleKey);
const toggles = reactive(
  Object.fromEntries(items.filter((item) => item.kind === "toggle").map((item) => [item.id, Boolean(item.enabled)])),
);
const host = ref("example.com");
const port = ref("443");
const remark = ref(title.value);
const uuid = ref("");
const sni = ref("");
const publicKey = ref("");
const shortId = ref("");
const spiderX = ref("");
const mldsa65Verify = ref("");
const flow = ref("xtls-rprx-vision");
const saveError = ref<string | null>(null);
const saving = ref(false);
const remotePrimary = ref(dnsState.remoteServers[0] ?? "1.1.1.1");
const remoteBackup = ref(dnsState.remoteServers[1] ?? "8.8.8.8");

const isDns = computed(
  () => props.screen === "dns" || props.titleKey === "settings.items.remote-dns.title",
);

const isVless = computed(
  () => props.screen === "vless" || props.titleKey === "settings.items.manual-endpoint.title",
);

function isValidShortId(value: string): boolean {
  const trimmed = value.trim();
  if (trimmed.length === 0) return true;
  if (trimmed.length > 16 || trimmed.length % 2 !== 0) return false;
  return /^[0-9a-fA-F]+$/.test(trimmed);
}

function itemTitle(labelKey: string | undefined): string {
  return labelKey ? t(labelKey) : "";
}

function itemSubtitle(labelKey: string | undefined): string | undefined {
  if (labelKey !== "nested.advanced") return undefined;
  return t("nested.advancedSubtitle", { title: title.value });
}

function itemValue(kind: string, value?: string): string | undefined {
  if (kind !== "value") return undefined;
  return value === "Auto" ? t("nested.modeValue") : value;
}

function openDeeper(labelKey: string): void {
  push({
    name: "nested-setting",
    title: t(labelKey),
    parentTitle: title.value,
    props: {
      titleKey: labelKey,
      parentTitleKey: props.titleKey,
      screen: "list",
    },
  });
}

async function onFallbackToggle(enabled: boolean): Promise<void> {
  saveError.value = null;
  try {
    await setFallbackToSystem(enabled);
  } catch (error) {
    saveError.value = error instanceof Error ? error.message : String(error);
  }
}

async function saveDns(): Promise<void> {
  saveError.value = null;
  if (dnsEditorsLocked.value) return;
  saving.value = true;
  try {
    await setRemoteServers([remotePrimary.value.trim(), remoteBackup.value.trim()].filter(Boolean));
    back();
  } catch (error) {
    saveError.value = error instanceof Error ? error.message : String(error);
  } finally {
    saving.value = false;
  }
}

async function saveVless(): Promise<void> {
  saveError.value = null;
  const portNumber = Number(port.value);
  if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
    saveError.value = t("nested.portInvalid");
    return;
  }
  if (!isValidShortId(shortId.value)) {
    saveError.value = t("nested.shortIdInvalid");
    return;
  }

  saving.value = true;
  try {
    const profile = {
      remark: remark.value.trim(),
      server: host.value.trim(),
      serverPort: portNumber,
      uuid: uuid.value.trim(),
      serverName: sni.value.trim(),
      publicKey: publicKey.value.trim(),
      shortId: shortId.value.trim(),
      spiderX: spiderX.value.trim() || null,
      mldsa65Verify: mldsa65Verify.value.trim() || null,
      flow: flow.value.trim() || null,
      fingerprint: "chrome",
    };
    await saveManualEndpoint(profile);
    await applyConnectedProfile(profile);
    back();
  } catch (error) {
    saveError.value = error instanceof Error ? error.message : String(error);
  } finally {
    saving.value = false;
  }
}

onMounted(async () => {
  if (!isVless.value) return;
  try {
    const profile = await loadManualEndpoint();
    if (!profile) return;
    host.value = profile.server;
    port.value = String(profile.serverPort);
    remark.value = profile.remark || remark.value;
    uuid.value = profile.uuid;
    sni.value = profile.serverName;
    publicKey.value = profile.publicKey;
    shortId.value = profile.shortId;
    spiderX.value = profile.spiderX ?? "";
    mldsa65Verify.value = profile.mldsa65Verify ?? "";
    flow.value = profile.flow ?? "";
  } catch (error) {
    saveError.value = error instanceof Error ? error.message : String(error);
  }
});
</script>

<template>
  <Screen :title="title" :parent-title="parentTitle" compact @back="back">
    <template v-if="isDns">
      <p class="hint">{{ t("nested.dnsHint") }}</p>
      <TextField
        v-model="remotePrimary"
        :label="t('nested.dnsServer')"
        :placeholder="t('nested.dnsServerPlaceholder')"
        :disabled="dnsEditorsLocked"
      />
      <TextField
        v-model="remoteBackup"
        :label="t('nested.dnsServerBackup')"
        :placeholder="t('nested.dnsServerBackupPlaceholder')"
        :disabled="dnsEditorsLocked"
      />
      <div class="fallback-row">
        <div class="fallback-title-row">
          <p class="fallback-title">{{ t("nested.dnsFallback") }}</p>
          <HelpTip :text="t('nested.dnsFallbackHint')" :label="t('nested.moreInfo')" />
        </div>
        <Switch
          :model-value="dnsState.fallbackToSystem"
          :disabled="dnsEditorsLocked"
          @update:model-value="onFallbackToggle"
        />
      </div>
      <p v-if="saveError" class="save-error">{{ saveError }}</p>
      <Button :disabled="saving || dnsEditorsLocked" @click="saveDns">{{ t("nested.save") }}</Button>
    </template>

    <template v-else-if="isVless">
      <p class="hint">{{ t("nested.vlessHint") }}</p>
      <TextField v-model="remark" :label="t('nested.remark')" :placeholder="t('nested.remarkPlaceholder')" />
      <TextField v-model="host" :label="t('nested.host')" :placeholder="t('nested.hostPlaceholder')" />
      <TextField v-model="port" :label="t('nested.port')" type="number" :placeholder="t('nested.portPlaceholder')" />
      <TextField v-model="uuid" :label="t('nested.uuid')" :placeholder="t('nested.uuidPlaceholder')" />
      <TextField v-model="sni" :label="t('nested.sni')" :placeholder="t('nested.sniPlaceholder')" />
      <TextField v-model="publicKey" :label="t('nested.publicKey')" :placeholder="t('nested.publicKeyPlaceholder')" />
      <TextField v-model="shortId" :label="t('nested.shortId')" :placeholder="t('nested.shortIdPlaceholder')" />
      <TextField v-model="spiderX" :label="t('nested.spiderX')" :placeholder="t('nested.spiderXPlaceholder')" />
      <TextField
        v-model="mldsa65Verify"
        :label="t('nested.mldsa65Verify')"
        :placeholder="t('nested.mldsa65VerifyPlaceholder')"
      />
      <TextField v-model="flow" :label="t('nested.flow')" :placeholder="t('nested.flowPlaceholder')" />
      <p v-if="saveError" class="save-error">{{ saveError }}</p>
      <Button :disabled="saving" @click="saveVless">{{ t("nested.save") }}</Button>
    </template>

    <template v-else-if="screen === 'fields'">
      <TextField v-model="host" :label="t('nested.host')" :placeholder="t('nested.hostPlaceholder')" />
      <TextField v-model="port" :label="t('nested.port')" type="number" :placeholder="t('nested.portPlaceholder')" />
      <TextField v-model="remark" :label="t('nested.remark')" :placeholder="t('nested.remarkPlaceholder')" />
      <Button @click="back">{{ t("nested.save") }}</Button>
    </template>

    <List v-else :title="title">
      <ListItem
        v-for="item in items"
        :key="item.id"
        :title="itemTitle(item.labelKey)"
        :subtitle="itemSubtitle(item.labelKey)"
        :icon-letter="itemTitle(item.labelKey).slice(0, 1)"
        :icon-color="item.iconColor"
        :value="itemValue(item.kind, item.value)"
        :chevron="item.kind !== 'toggle'"
        :clickable="item.kind !== 'toggle'"
        @click="item.kind !== 'toggle' && item.labelKey ? openDeeper(item.labelKey) : undefined"
      >
        <template v-if="item.kind === 'toggle'" #trailing>
          <Switch v-model="toggles[item.id]" />
        </template>
      </ListItem>
    </List>
  </Screen>
</template>

<style scoped>
.save-error {
  margin: 0 4px 12px;
  color: var(--color-danger, #c0392b);
  font-size: 13px;
}

.hint {
  margin: 0 4px 12px;
  color: var(--color-muted);
  font-size: 13px;
  line-height: 1.4;
}

.fallback-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 0 4px 16px;
}

.fallback-title-row {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 2px;
  min-width: 0;
}

.fallback-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
</style>
