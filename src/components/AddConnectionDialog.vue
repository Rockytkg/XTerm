<script setup>
import "../styles/connection-dialog.scss";
import { computed, reactive, ref, useSlots, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  DialogTrigger,
} from "reka-ui";
import { CirclePlus, SquarePen, X } from "@lucide/vue";
import { createCredential, loadCredentials } from "../services/credentials";
import { invokeDebugIpc } from "../services/ipc/core";
import { usePrivateKeyPicker } from "../composables/usePrivateKeyPicker";
import { createConnection, getConnection, updateConnectionProfile } from "../services/workspace";
import {
  CONNECTION_PROTOCOLS,
  isSerialProtocol,
  requiresPasswordCredential,
  requiresHostKeyVerification,
  supportsSavedCredential,
} from "../utils/connectionProtocols";
import { createLogger } from "../utils/logger";
import ConnectionProtocolPicker from "./connection-dialog/ConnectionProtocolPicker.vue";
import ConnectionSessionOptions from "./connection-dialog/ConnectionSessionOptions.vue";
import JumpHostEditorDialog from "./connection-dialog/JumpHostEditorDialog.vue";
import SerialConnectionFields from "./connection-dialog/SerialConnectionFields.vue";
import SshConnectionFields from "./connection-dialog/SshConnectionFields.vue";
import TelnetConnectionFields from "./connection-dialog/TelnetConnectionFields.vue";
import {
  buildConnectionProfile,
  createProtocolDraft,
  resetProtocolDrafts,
} from "./connection-dialog/connectionDialogModel";

const props = defineProps({
  editConnection: { type: Object, default: null },
  connections: { type: Array, default: () => [] },
  open: { type: Boolean, default: undefined },
});
const emit = defineEmits(["save", "update:open"]);
const { t } = useI18n();
const slots = useSlots();
const logger = createLogger("frontend.connection.add_dialog");

const internalOpen = ref(false);
const dialogOpen = computed({
  get: () => props.open ?? internalOpen.value,
  set(value) {
    internalOpen.value = value;
    emit("update:open", value);
  },
});
const activeProtocol = ref("ssh");
const jumpHostEditorOpen = ref(false);
// Stays false until the edit profile and credentials have loaded, so the
// form is first rendered in its final state instead of flashing from
// "password" to "saved credential" when async data arrives.
const dialogReady = ref(false);
const loadedEditProfile = ref(null);
let dialogLoadToken = 0;

const { pickPrivateKey, keepDialogOpen: keepDialogOpenForPrivateKeyPicker } =
  usePrivateKeyPicker();

const protocolDrafts = reactive({
  ssh: createProtocolDraft("ssh"),
  telnet: createProtocolDraft("telnet"),
  serial: createProtocolDraft("serial"),
});
const fieldErrors = reactive({
  ssh: {},
  telnet: {},
  serial: {},
});
const sessionOptionsOpen = reactive({
  ssh: false,
  telnet: false,
  serial: false,
});

const detectedSerialPorts = ref([]);
const serialPortsLoading = ref(false);
const credentials = ref([]);

const activeForm = computed(() => protocolDrafts[activeProtocol.value]);
const activeErrors = computed(() => fieldErrors[activeProtocol.value]);
const isEdit = computed(() => !!props.editConnection);
const isSSH = computed(() => requiresHostKeyVerification(activeProtocol.value));
const isSerial = computed(() => isSerialProtocol(activeProtocol.value));
const activeProtocolSupportsCredentials = computed(() =>
  supportsSavedCredential(activeProtocol.value),
);

const serialPortOptions = computed(() => [
  { label: t("connectionDialog.auto"), value: "auto" },
  ...detectedSerialPorts.value.map((port) => ({
    label: port.label || port.name,
    value: port.name,
  })),
]);
const baudRateOptions = computed(() => [
  { label: t("connectionDialog.auto"), value: "auto" },
  300,
  1200,
  2400,
  4800,
  9600,
  14400,
  19200,
  38400,
  57600,
  115200,
  230400,
  460800,
  921600,
  1000000,
  2000000,
]);
const dataBitsOptions = [5, 6, 7, 8];
const stopBitsOptions = [1, 2];
const parityOptions = computed(() => [
  { label: t("connectionDialog.serialParity.none"), value: "none" },
  { label: t("connectionDialog.serialParity.odd"), value: "odd" },
  { label: t("connectionDialog.serialParity.even"), value: "even" },
]);
const flowControlOptions = computed(() => [
  { label: t("connectionDialog.serialFlowControl.none"), value: "none" },
  { label: t("connectionDialog.serialFlowControl.software"), value: "software" },
  { label: t("connectionDialog.serialFlowControl.hardware"), value: "hardware" },
]);

const filteredCredentials = computed(() => {
  if (!activeProtocolSupportsCredentials.value) return [];
  if (requiresPasswordCredential(activeProtocol.value)) {
    return credentials.value.filter((credential) => credential.credType === "password");
  }
  return credentials.value;
});
const selectedCredential = computed(() => {
  if (!activeProtocolSupportsCredentials.value) return null;
  return (
    filteredCredentials.value.find(
      (credential) => credential.id === activeForm.value.savedCredentialId,
    ) ?? null
  );
});

watch(
  dialogOpen,
  (value) => {
    if (value) {
      void initializeDialog();
    } else {
      dialogLoadToken += 1;
      dialogReady.value = false;
      jumpHostEditorOpen.value = false;
      serialPortsLoading.value = false;
    }
  },
  { immediate: true },
);

function isCurrentDialogLoad(token) {
  return dialogOpen.value && token === dialogLoadToken;
}

function resetSessionOptionsState() {
  CONNECTION_PROTOCOLS.forEach((protocol) => {
    sessionOptionsOpen[protocol] = false;
  });
}

function clearErrors(protocol = activeProtocol.value) {
  Object.keys(fieldErrors[protocol]).forEach((key) => delete fieldErrors[protocol][key]);
}

function clearAllErrors() {
  CONNECTION_PROTOCOLS.forEach((protocol) => clearErrors(protocol));
}

async function initializeDialog() {
  const token = ++dialogLoadToken;
  dialogReady.value = false;
  resetSessionOptionsState();
  credentials.value = [];
  detectedSerialPorts.value = [];
  serialPortsLoading.value = false;
  loadedEditProfile.value = null;

  let fullProfile = props.editConnection;
  let loadError = null;
  try {
    if (props.editConnection?.id) {
      fullProfile = (await getConnection(props.editConnection.id)) ?? props.editConnection;
    }
  } catch (error) {
    loadError = String(error);
  }

  if (!isCurrentDialogLoad(token)) return;
  loadedEditProfile.value = fullProfile ?? null;
  activeProtocol.value = resetProtocolDrafts(protocolDrafts, fullProfile);
  clearAllErrors();
  if (loadError) {
    fieldErrors[activeProtocol.value].name = loadError;
  }
  // Await credentials (and the default-credential selection they trigger)
  // before revealing the form, so the auth-mode highlight renders once in
  // its final state instead of flickering.
  await loadProtocolResources(activeProtocol.value, token);
  if (isCurrentDialogLoad(token)) {
    dialogReady.value = true;
  }
}

function selectProtocol(protocol) {
  if (isEdit.value || protocol === activeProtocol.value) return;
  activeProtocol.value = protocol;
  void loadProtocolResources(protocol, dialogLoadToken);
}

async function loadProtocolResources(protocol, token) {
  if (supportsSavedCredential(protocol)) {
    await loadCredentialsForProtocol(protocol, token);
  } else if (isCurrentDialogLoad(token) && activeProtocol.value === protocol) {
    credentials.value = [];
  }

  if (isSerialProtocol(protocol)) {
    await loadSerialPortsForProtocol(protocol, token);
  } else if (isCurrentDialogLoad(token) && activeProtocol.value === protocol) {
    detectedSerialPorts.value = [];
    serialPortsLoading.value = false;
  }
}

async function loadCredentialsForProtocol(protocol, token) {
  try {
    const value = await loadCredentials();
    if (!isCurrentDialogLoad(token) || activeProtocol.value !== protocol) return;
    credentials.value = Array.isArray(value) ? value : [];
    selectDefaultCredentialIfAvailable();
  } catch (error) {
    if (!isCurrentDialogLoad(token) || activeProtocol.value !== protocol) return;
    credentials.value = [];
    fieldErrors[protocol].savedCredentialId = String(error);
  }
}

function validateConnection(commitErrors = true) {
  const protocol = activeProtocol.value;
  const form = protocolDrafts[protocol];
  const nextErrors = {};
  const name = form.name.trim();
  const host = form.host?.trim?.() ?? "";
  const user = form.user?.trim?.() ?? "";
  const port = Number(form.port);

  if (!name) nextErrors.name = t("connectionDialog.validation.nameRequired");

  if (isSerialProtocol(protocol)) {
    if (!form.serialPort)
      nextErrors.serialPort = t("connectionDialog.validation.serialPortRequired");
    if (
      form.baudRate !== "auto" &&
      (!Number.isInteger(Number(form.baudRate)) || Number(form.baudRate) <= 0)
    ) {
      nextErrors.baudRate = t("connectionDialog.validation.baudRateInvalid");
    }
    if (!dataBitsOptions.includes(Number(form.dataBits))) {
      nextErrors.dataBits = t("connectionDialog.validation.serialLineInvalid");
    }
    if (!stopBitsOptions.includes(Number(form.stopBits))) {
      nextErrors.stopBits = t("connectionDialog.validation.serialLineInvalid");
    }
    if (!["none", "odd", "even"].includes(String(form.parity))) {
      nextErrors.parity = t("connectionDialog.validation.serialLineInvalid");
    }
    if (!["none", "software", "hardware"].includes(String(form.flowControl))) {
      nextErrors.flowControl = t("connectionDialog.validation.serialLineInvalid");
    }
  } else {
    if (!host) nextErrors.host = t("connectionDialog.validation.hostRequired");
    else if (/\s/.test(host)) nextErrors.host = t("connectionDialog.validation.hostInvalid");
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      nextErrors.port = t("connectionDialog.validation.portInvalid");
    }

    if (requiresHostKeyVerification(protocol)) {
      if (!user) nextErrors.user = t("connectionDialog.validation.userRequired");
      const jumpHosts = Array.isArray(form.jumpHosts) ? form.jumpHosts : [];
      const currentConnectionId = (
        loadedEditProfile.value?.id ||
        props.editConnection?.id ||
        ""
      ).trim();
      const availableJumpConnectionIds = new Set(
        props.connections
          .filter((connection) => requiresHostKeyVerification(connection.protocol))
          .map((connection) => connection.id)
          .filter((id) => id && id !== currentConnectionId),
      );
      for (const hop of jumpHosts) {
        const source = hop?.source || (hop?.connectionId ? "connection" : "manual");
        const connectionId = hop?.connectionId?.trim?.() || "";
        const hopHost = hop?.host?.trim?.() || "";
        if (source === "connection") {
          if (!connectionId) {
            nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostSelectionRequired");
            break;
          }
          if (connectionId === currentConnectionId) {
            nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostSelfReference");
            break;
          }
          if (props.connections.length && !availableJumpConnectionIds.has(connectionId)) {
            nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostSelectionRequired");
            break;
          }
        } else if (!hopHost) {
          nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostsRequired");
          break;
        } else if (!hop?.user?.trim?.()) {
          nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostUsernameRequired");
          break;
        } else if (!hop?.savedCredentialId?.trim?.()) {
          const authMethod = hop?.authMethod || "password";
          if (authMethod === "key") {
            if (!hop?.privateKey?.trim?.()) {
              nextErrors.jumpHosts = t("connectionDialog.validation.jumpHostCredentialRequired");
              break;
            }
          }
        }
      }
      if (form.authMethod === "key" && !form.savedCredentialId && !form.privateKey.trim()) {
        nextErrors.savedCredentialId = t("connectionDialog.validation.keyRequired");
      }
    }
  }

  if (commitErrors) {
    clearErrors(protocol);
    Object.assign(fieldErrors[protocol], nextErrors);
  }

  return nextErrors;
}

function clearFieldError(field) {
  delete activeErrors.value[field];
}

function selectDefaultCredentialIfAvailable() {
  if (!isSSH.value || activeForm.value.savedCredentialId) return;
  if (activeForm.value.password?.trim?.() || activeForm.value.privateKey?.trim?.()) return;
  const credential = filteredCredentials.value[0];
  if (credential) {
    onCredentialSelect(credential.id);
  }
}

function updateActiveField(field, value) {
  activeForm.value[field] = value;
}

function normalizePort() {
  const form = activeForm.value;
  const numeric = Number.parseInt(String(form.port), 10);
  form.port = Number.isFinite(numeric) ? Math.min(Math.max(numeric, 1), 65535) : "";
}

function onPortInput() {
  const form = activeForm.value;
  form.port = String(form.port).replace(/\D/g, "").slice(0, 5);
  clearFieldError("port");
}

function onCredentialSelect(credentialId) {
  const form = activeForm.value;
  form.savedCredentialId = credentialId;
  const credential = credentials.value.find((item) => item.id === credentialId);
  if (!credential) return;
  form.authMethod = isSSH.value ? credential.credType : "password";
  clearFieldError("password");
  clearFieldError("savedCredentialId");
  clearFieldError("user");
}

function onAuthMethodChange(method) {
  const form = activeForm.value;
  const methodChanged = form.authMethod !== method;
  if (!methodChanged && !form.savedCredentialId) return;
  form.authMethod = method;
  form.savedCredentialId = "";
  if (methodChanged) {
    form.password = "";
    form.privateKey = "";
    form.keyPassphrase = "";
  }
  clearFieldError("password");
  clearFieldError("savedCredentialId");
}

async function pickPrivateKeyFile() {
  try {
    const privateKey = await pickPrivateKey(t("credentials.fields.choosePrivateKeyTitle"));
    if (!privateKey) return;
    activeForm.value.privateKey = privateKey;
    clearFieldError("savedCredentialId");
  } catch (error) {
    activeErrors.value.savedCredentialId = String(error);
  }
}

async function ensureInlineCredential() {
  if (!activeProtocolSupportsCredentials.value || activeForm.value.savedCredentialId) {
    return activeForm.value.savedCredentialId || undefined;
  }

  const form = activeForm.value;
  if (requiresPasswordCredential(activeProtocol.value) && !form.password?.trim?.()) {
    return undefined;
  }
  const baseName = form.name.trim() || form.host.trim();
  const credential =
    isSSH.value && form.authMethod === "key"
      ? await createCredential({
          credType: "key",
          name: `${baseName} key`,
          privateKey: form.privateKey,
          passphrase: form.keyPassphrase,
        })
      : await createCredential({
          credType: "password",
          name: `${baseName} password`,
          password: form.password,
        });

  credentials.value = [...credentials.value, credential];
  form.savedCredentialId = credential.id;
  form.password = "";
  form.privateKey = "";
  form.keyPassphrase = "";
  return credential.id;
}

async function ensureJumpHostInlineCredentials() {
  if (!isSSH.value || !Array.isArray(activeForm.value.jumpHosts)) return;

  const form = activeForm.value;
  const nextJumpHosts = [];
  const createdCredentials = [];

  for (const hop of form.jumpHosts) {
    const source = hop.source || (hop.connectionId ? "connection" : "manual");
    if (source !== "manual" || hop.savedCredentialId) {
      nextJumpHosts.push(hop);
      continue;
    }

    const baseName = hop.host?.trim?.() || form.name.trim() || form.host.trim() || "jump host";
    const authMethod = hop.authMethod || "password";
    const credential =
      authMethod === "key"
        ? await createCredential({
            credType: "key",
            name: `${baseName} jump key`,
            privateKey: hop.privateKey,
            passphrase: hop.keyPassphrase,
          })
        : await createCredential({
            credType: "password",
            name: `${baseName} jump password`,
            password: hop.password ?? "",
          });

    createdCredentials.push(credential);
    nextJumpHosts.push({
      ...hop,
      authMethod: credential.credType,
      savedCredentialId: credential.id,
      password: "",
      privateKey: "",
      keyPassphrase: "",
    });
  }

  if (createdCredentials.length) {
    credentials.value = [...credentials.value, ...createdCredentials];
    form.jumpHosts = nextJumpHosts;
  }
}

async function loadSerialPorts() {
  return loadSerialPortsForProtocol("serial", dialogLoadToken);
}

async function loadSerialPortsForProtocol(protocol, token) {
  if (!isSerialProtocol(protocol)) return;
  serialPortsLoading.value = true;
  try {
    let ports = await invokeDebugIpc("serial_list_ports");
    if (!isCurrentDialogLoad(token) || activeProtocol.value !== protocol) return;
    ports = Array.isArray(ports) ? ports : [];
    const serialPort = protocolDrafts.serial.serialPort;
    if (serialPort !== "auto" && !ports.some((port) => port.name === serialPort)) {
      ports = [{ name: serialPort, label: serialPort, kind: "saved" }, ...ports];
    }
    detectedSerialPorts.value = ports;
  } catch (error) {
    if (!isCurrentDialogLoad(token) || activeProtocol.value !== protocol) return;
    detectedSerialPorts.value = [];
    fieldErrors.serial.serialPort = String(error);
  } finally {
    if (isCurrentDialogLoad(token) && activeProtocol.value === protocol) {
      serialPortsLoading.value = false;
    }
  }
}

async function saveConnection() {
  if (Object.keys(validateConnection()).length > 0) return;
  logger.info("connection.save", {
    name: activeForm.value.name,
    protocol: activeProtocol.value,
    mode: isEdit.value ? "edit" : "new",
  });

  let savedCredentialId;
  try {
    savedCredentialId = await ensureInlineCredential();
  } catch (error) {
    activeErrors.value.savedCredentialId = String(error);
    return;
  }
  try {
    await ensureJumpHostInlineCredentials();
  } catch (error) {
    activeErrors.value.jumpHosts = String(error);
    return;
  }

  const id = props.editConnection?.id ?? "";
  const profile = buildConnectionProfile({
    baseProfile: loadedEditProfile.value ?? props.editConnection ?? {},
    id,
    protocol: activeProtocol.value,
    form: activeForm.value,
    savedCredentialId,
  });

  try {
    let savedId = id;
    if (props.editConnection?.id) {
      await updateConnectionProfile(id, profile);
    } else {
      savedId = await createConnection(profile);
    }
    emit("save", { id: savedId });
    clearAllErrors();
    dialogOpen.value = false;
  } catch (error) {
    activeErrors.value.name = String(error);
  }
}
</script>

<template>
  <DialogRoot v-model:open="dialogOpen">
    <DialogTrigger
      v-if="slots.default"
      as-child
    >
      <slot />
    </DialogTrigger>
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent
        class="dialog-content conn-dialog focus:outline-none"
        @pointer-down-outside="keepDialogOpenForPrivateKeyPicker"
        @interact-outside="keepDialogOpenForPrivateKeyPicker"
      >
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <component
              :is="isEdit ? SquarePen : CirclePlus"
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ isEdit ? t("connectionDialog.titleEdit") : t("connectionDialog.title") }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ t("connectionDialog.description") }}
            </DialogDescription>
          </div>
          <DialogClose as-child>
            <button
              type="button"
              class="ui-icon-button shrink-0"
              :aria-label="t('actions.closeDialog')"
            >
              <X
                :size="15"
                stroke-width="1.8"
              />
            </button>
          </DialogClose>
        </header>

        <form
          v-if="dialogReady"
          class="conn-dialog-body"
          autocomplete="off"
          @submit.prevent="saveConnection"
        >
          <ConnectionProtocolPicker
            v-if="!isEdit"
            :model-value="activeProtocol"
            :protocols="CONNECTION_PROTOCOLS"
            @update:model-value="selectProtocol"
          />

          <SshConnectionFields
            v-if="isSSH"
            :form="activeForm"
            :errors="activeErrors"
            :filtered-credentials="filteredCredentials"
            :jump-host-error="activeErrors.jumpHosts"
            @auth-method-change="onAuthMethodChange"
            @clear-field="clearFieldError"
            @credential-select="onCredentialSelect"
            @open-jump-editor="jumpHostEditorOpen = true"
            @normalize-port="normalizePort"
            @pick-private-key="pickPrivateKeyFile"
            @port-input="onPortInput"
            @update-field="updateActiveField"
          />

          <SerialConnectionFields
            v-else-if="isSerial"
            :form="activeForm"
            :errors="activeErrors"
            :filtered-credentials="filteredCredentials"
            :selected-credential="selectedCredential"
            :serial-port-options="serialPortOptions"
            :baud-rate-options="baudRateOptions"
            :data-bits-options="dataBitsOptions"
            :stop-bits-options="stopBitsOptions"
            :parity-options="parityOptions"
            :flow-control-options="flowControlOptions"
            :loading="serialPortsLoading"
            @auth-method-change="onAuthMethodChange"
            @clear-field="clearFieldError"
            @credential-select="onCredentialSelect"
            @refresh-serial-ports="loadSerialPorts"
            @update-field="updateActiveField"
          />

          <TelnetConnectionFields
            v-else
            :form="activeForm"
            :errors="activeErrors"
            :filtered-credentials="filteredCredentials"
            :selected-credential="selectedCredential"
            @auth-method-change="onAuthMethodChange"
            @clear-field="clearFieldError"
            @credential-select="onCredentialSelect"
            @normalize-port="normalizePort"
            @port-input="onPortInput"
            @update-field="updateActiveField"
          />

          <ConnectionSessionOptions
            :form="activeForm"
            :protocol="activeProtocol"
            :open="sessionOptionsOpen[activeProtocol]"
            @update:open="sessionOptionsOpen[activeProtocol] = $event"
            @update-field="updateActiveField"
          />
        </form>
        <div
          v-else
          class="conn-dialog-body min-h-[280px]"
          aria-hidden="true"
        />

        <JumpHostEditorDialog
          v-if="isSSH"
          :open="jumpHostEditorOpen"
          :jump-hosts="activeForm.jumpHosts"
          :connections="props.connections"
          :credentials="credentials"
          :current-connection-id="loadedEditProfile?.id || props.editConnection?.id || ''"
          :error="activeErrors.jumpHosts"
          @update:open="jumpHostEditorOpen = $event"
          @update:jump-hosts="updateActiveField('jumpHosts', $event)"
        />

        <footer class="conn-dialog-footer">
          <DialogClose as-child>
            <button
              type="button"
              class="ui-button-secondary"
            >
              {{ t("actions.cancel") }}
            </button>
          </DialogClose>
          <button
            type="button"
            class="ui-button-primary"
            :disabled="!dialogReady"
            @click="saveConnection"
          >
            {{ isEdit ? t("actions.save") : t("actions.addConnection") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
