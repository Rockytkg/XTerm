<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import cytoscape from "cytoscape";
import { GitBranch, Link2, Minus, Plus, RefreshCw, SlidersHorizontal, Trash2 } from "@lucide/vue";
import AppTooltip from "../components/AppTooltip.vue";
import "../styles/views-relationship.scss";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import {
  clearCredentialReferences,
  deleteCredential,
  loadCredentialUsages,
  loadCredentials,
} from "../services/credentials";
import {
  clearConnectionSavedCredential,
  loadWorkspaceBootstrap,
  setConnectionSavedCredential,
} from "../services/workspace";
import { useAppPreferences } from "../composables/useAppPreferences";
import { useDialogExitTeardown } from "../composables/useDialogExitTeardown";
import {
  normalizeCredentialUsages,
  useCredentialDeleteFlow,
} from "../composables/useCredentialDeleteFlow";
import { openContextMenu } from "../services/contextMenu";
import { useToasts } from "../composables/useToasts";
import { useWorkspaceStore } from "../stores/workspaceStore";
import {
  connectionEndpointLabel,
  isSerialProtocol,
  isTelnetProtocol,
  supportsSavedCredential,
} from "../utils/connectionProtocols";
import { isPrimaryModifier } from "../utils/platform";

const props = defineProps({
  embedded: { type: Boolean, default: false },
});
const emit = defineEmits(["state-changed"]);

const { t } = useI18n();
const router = useRouter();
const { showToast } = useToasts();
const { resolvedTheme } = useAppPreferences();
const workspace = useWorkspaceStore();

const graphEl = ref(null);
const credentials = ref([]);
const connections = ref([]);
const credentialUsagesList = ref([]);
const graphError = ref("");
const loading = ref(false);
const pendingRelation = ref(null);
const pendingBulkCredentialDelete = ref(null);
const bulkCredentialDeleteOpen = ref(false);
const relationBusy = ref(false);
const relationDrawMode = ref(false);
const { scheduleExitTeardown, cancelExitTeardown } = useDialogExitTeardown();

let cy = null;
let unmounted = false;
let renderFrame = 0;
let themeRefreshFrame = 0;
let themeRefreshTimer = 0;
let layoutFitTimer = 0;
let themeObserver = null;
let renderNeedsFit = false;
let activeDraftRelation = null;
let selectionMenuTimer = 0;
let selectionMenuPointer = null;

const NODE_SIZE = Object.freeze({ width: 292, height: 86 });
const DRAFT_TARGET_NODE_ID = "__relationship-draft-target";
const DRAFT_EDGE_ID = "__relationship-draft-edge";
const SELECTION_MENU_HOLD_MS = 520;
const SELECTION_MENU_MOVE_TOLERANCE = 10;

const graphConnections = computed(() =>
  connections.value.filter((connection) => isCredentialConnection(connection)),
);
const graphConnectionIds = computed(
  () => new Set(graphConnections.value.map((connection) => connection.id)),
);
const topology = computed(() => buildCredentialTopology());
const topologyNodeMap = computed(
  () => new Map(topology.value.nodes.map((node) => [node.id, node.properties])),
);
const stats = computed(() => {
  const nodes = topology.value.nodes;
  return {
    credentials: nodes.filter((node) => node.properties?.category === "credential").length,
    connections: nodes.filter((node) => node.properties?.kind === "connection").length,
    edges: topology.value.edges.length,
  };
});
const pendingConfirmText = computed(() => {
  const relation = pendingRelation.value;
  if (!relation) return "";
  if (relation.action === "bulk-remove") {
    return t("relationshipGraph.confirm.credential.bulkRemove", {
      count: relation.relations.length,
    });
  }
  const action = relation.action === "create" ? "create" : "remove";
  return t(`relationshipGraph.confirm.credential.${action}`, {
    source: relation.sourceName,
    target: relation.targetName,
  });
});
const pendingConfirmTone = computed(() =>
  pendingRelation.value?.action === "remove" || pendingRelation.value?.action === "bulk-remove"
    ? "danger"
    : "info",
);
const pendingConfirmIcon = computed(() =>
  pendingRelation.value?.action === "remove" || pendingRelation.value?.action === "bulk-remove"
    ? Trash2
    : Link2,
);
const pendingBulkCredentialDeleteDescription = computed(() => {
  const pending = pendingBulkCredentialDelete.value;
  if (!pending) return "";
  return t("relationshipGraph.confirm.credentialDelete.bulkDescription", {
    count: pending.credentials.length,
    usageCount: pending.usageCount,
  });
});
const {
  credentialDeleteOpen,
  pendingCredentialDelete,
  credentialDeleteBusy,
  pendingCredentialDeleteDescription,
  requestCredentialDelete,
  confirmCredentialDelete,
  cancelCredentialDelete,
} = useCredentialDeleteFlow({
  t,
  showToast,
  getUsages: (credentialId) => credentialUsages(credentialId),
  async onDeleted() {
    await refreshGraph();
    emit("state-changed");
  },
  async onFailed() {
    await refreshGraph();
  },
});

onMounted(async () => {
  await loadGraphState();
  if (unmounted) return;
  await nextTick();
  if (unmounted) return;
  initGraph();
  startThemeObserver();
});

onBeforeUnmount(() => {
  unmounted = true;
});

onUnmounted(() => {
  if (renderFrame) window.cancelAnimationFrame(renderFrame);
  window.clearTimeout(layoutFitTimer);
  if (themeRefreshFrame) window.cancelAnimationFrame(themeRefreshFrame);
  window.clearTimeout(themeRefreshTimer);
  cancelSelectionMenuTimer();
  themeObserver?.disconnect();
  themeObserver = null;
  destroyGraph();
});

async function loadGraphState() {
  loading.value = true;
  graphError.value = "";
  try {
    const [savedCredentials, savedUsages, workspace] = await Promise.all([
      loadCredentials(),
      loadCredentialUsages(),
      loadWorkspaceBootstrap(),
    ]);
    credentials.value = Array.isArray(savedCredentials) ? savedCredentials : [];
    credentialUsagesList.value = Array.isArray(savedUsages) ? savedUsages : [];
    const loadedConnections = Array.isArray(workspace?.connections) ? workspace.connections : [];
    connections.value = loadedConnections.filter((connection) => isCredentialConnection(connection));
  } catch (error) {
    graphError.value = String(error);
  } finally {
    loading.value = false;
  }
}

function initGraph() {
  if (!graphEl.value || cy) return;

  cy = cytoscape({
    container: graphEl.value,
    elements: [],
    boxSelectionEnabled: true,
    maxZoom: 2.8,
    minZoom: 0.25,
    selectionType: "additive",
    style: graphStyles(),
  });

  bindGraphEvents();
  renderGraph({ fit: true });
}

function graphStyles() {
  const bgSecondary = cssVar("--bg-secondary");
  const bgPrimary = cssVar("--bg-primary");
  const border = cssVar("--border");
  const textPrimary = cssVar("--text-primary");
  const textSecondary = cssVar("--text-secondary");
  const accent = cssVar("--accent");
  const accentLight = cssVar("--accent-light");
  const info = cssVar("--info");
  const infoBg = cssVar("--info-bg");
  const success = cssVar("--success");
  const successBg = cssVar("--success-bg");

  return [
    {
      selector: "node",
      style: {
        shape: "round-rectangle",
        width: NODE_SIZE.width,
        height: NODE_SIZE.height,
        "background-color": bgSecondary,
        "background-image": (node) => node.data("cardSvg") || "none",
        "background-fit": "cover",
        "background-width": NODE_SIZE.width,
        "background-height": NODE_SIZE.height,
        "background-clip": "none",
        "background-opacity": 1,
        "border-color": border,
        "border-width": 0,
        label: "",
        "font-family": "Ubuntu, Cantarell, Noto Sans, Inter, ui-sans-serif, system-ui, sans-serif",
        color: textPrimary,
        "overlay-opacity": 0,
        events: "yes",
      },
    },
    {
      selector: "node:selected, node.is-hovered",
      style: {
        "border-color": accent,
        "border-width": 2,
      },
    },
    {
      selector: "edge",
      style: {
        width: 2.4,
        "curve-style": "bezier",
        "line-color": info,
        "target-arrow-color": info,
        "target-arrow-shape": "triangle",
        "arrow-scale": 1,
        color: textSecondary,
        label: "data(label)",
        "font-family": "Ubuntu, Cantarell, Noto Sans, Inter, ui-sans-serif, system-ui, sans-serif",
        "font-size": 14,
        "font-weight": "normal",
        "text-background-color": bgPrimary,
        "text-background-opacity": 0.86,
        "text-background-padding": 5,
        "text-rotation": "autorotate",
        "overlay-opacity": 0,
      },
    },
    {
      selector: "edge:selected",
      style: {
        width: 2.8,
        "line-color": accent,
        "target-arrow-color": accent,
      },
    },
    {
      selector: ".relationship-draft-edge",
      style: {
        width: 2,
        "line-color": accent,
        "target-arrow-color": accent,
        "line-style": "dashed",
        opacity: 0.72,
        label: "",
        "target-arrow-shape": "none",
      },
    },
    {
      selector: ".relationship-draft-target",
      style: {
        width: 1,
        height: 1,
        "background-opacity": 0,
        "border-opacity": 0,
        opacity: 0,
      },
    },
    {
      selector: 'node[kind = "connection"]',
      style: {
        "background-color": infoBg,
        "border-color": info,
      },
    },
    {
      selector: 'node[kind = "key-credential"]',
      style: {
        "background-color": accentLight,
        "border-color": accent,
      },
    },
    {
      selector: 'node[kind = "password-credential"]',
      style: {
        "background-color": successBg,
        "border-color": success,
      },
    },
  ];
}

function destroyGraph() {
  cancelDraftRelation();
  cy?.destroy();
  cy = null;
}

function bindGraphEvents() {
  cy.on("mouseover", "node", (event) => {
    event.target.addClass("is-hovered");
  });
  cy.on("mouseout", "node", (event) => {
    event.target.removeClass("is-hovered");
  });
  cy.on("dbltap", "edge", (event) => {
    if (event.target.hasClass("relationship-draft-edge")) return;
    requestRemoveRelation(toTopologyEdgeFromCy(event.target, true));
  });
  cy.on("dbltap", "node", (event) => {
    if (relationDrawMode.value) return;
    openNodeTarget(event.target.data());
  });
  cy.on("mousedown", "node", (event) => {
    if (!relationDrawMode.value) return;
    if (event.originalEvent?.button !== 0) return;
    if (!canStartRelationFromNode(event.target.data())) return;
    startDraftRelation(event.target, event.position);
  });
  cy.on("mousedown", (event) => {
    maybeScheduleSelectionMenu(event);
  });
  cy.on("mousemove", (event) => {
    cancelSelectionMenuOnMove(event);
  });
  cy.on("mousemove", (event) => {
    if (!activeDraftRelation) return;
    updateDraftRelation(event.position);
  });
  cy.on("mouseup", cancelSelectionMenuTimer);
  cy.on("mouseup", (event) => {
    if (!activeDraftRelation) return;
    finishDraftRelation(event.position);
  });
  cy.on("tapstart", (event) => {
    maybeScheduleSelectionMenu(event);
  });
  cy.on("tapdrag", (event) => {
    cancelSelectionMenuOnMove(event);
  });
  cy.on("tapend", cancelSelectionMenuTimer);
}

function maybeScheduleSelectionMenu(event) {
  const original = event.originalEvent;
  if (!original || relationDrawMode.value) return;
  if (!isPrimaryModifier(original) || original.button !== 0) return;
  if (!selectedElements().length) return;
  cancelSelectionMenuTimer();
  selectionMenuPointer = {
    clientX: original.clientX,
    clientY: original.clientY,
    target: original.target,
  };
  selectionMenuTimer = window.setTimeout(() => {
    const pointer = selectionMenuPointer;
    cancelSelectionMenuTimer();
    if (!pointer?.target || !selectedElements().length) return;
    pointer.target.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        view: window,
        clientX: pointer.clientX,
        clientY: pointer.clientY,
        ctrlKey: true,
      }),
    );
  }, SELECTION_MENU_HOLD_MS);
}

function cancelSelectionMenuOnMove(event) {
  if (!selectionMenuPointer) return;
  const original = event.originalEvent;
  if (!original) return;
  const distance = Math.hypot(
    original.clientX - selectionMenuPointer.clientX,
    original.clientY - selectionMenuPointer.clientY,
  );
  if (distance > SELECTION_MENU_MOVE_TOLERANCE) cancelSelectionMenuTimer();
}

function cancelSelectionMenuTimer() {
  window.clearTimeout(selectionMenuTimer);
  selectionMenuTimer = 0;
  selectionMenuPointer = null;
}

function startDraftRelation(sourceNode, position) {
  cancelDraftRelation();
  activeDraftRelation = { sourceNodeId: sourceNode.id() };
  cy.batch(() => {
    sourceNode.select();
    cy.add([
      {
        group: "nodes",
        data: { id: DRAFT_TARGET_NODE_ID, label: "" },
        position,
        classes: "relationship-draft-target",
        selectable: false,
        grabbable: false,
      },
      {
        group: "edges",
        data: {
          id: DRAFT_EDGE_ID,
          source: sourceNode.id(),
          target: DRAFT_TARGET_NODE_ID,
          label: "",
          relation: "draft",
        },
        classes: "relationship-draft-edge",
        selectable: false,
      },
    ]);
  });
}

function updateDraftRelation(position) {
  const target = cy?.$id(DRAFT_TARGET_NODE_ID);
  if (!target?.length) return;
  target.position(position);
}

function finishDraftRelation(position) {
  const sourceNodeId = activeDraftRelation?.sourceNodeId;
  const targetNodeId = nodeIdAtPosition(position, sourceNodeId);
  cancelDraftRelation();
  if (!sourceNodeId || !targetNodeId || sourceNodeId === targetNodeId) return;
  disableRelationDrawMode();
  requestCreateRelation(edgeFromCyNodes(sourceNodeId, targetNodeId));
}

function nodeIdAtPosition(position, excludedNodeId = "") {
  if (!position || !cy) return "";
  const candidates = cy.nodes().filter((node) => {
    if (node.id() === excludedNodeId || node.id() === DRAFT_TARGET_NODE_ID) return false;
    const box = node.boundingBox({ includeLabels: false, includeOverlays: false });
    return (
      position.x >= box.x1 && position.x <= box.x2 && position.y >= box.y1 && position.y <= box.y2
    );
  });
  return candidates[0]?.id?.() || "";
}

function cancelDraftRelation() {
  if (!cy) {
    activeDraftRelation = null;
    return;
  }
  cy.batch(() => {
    cy.$id(DRAFT_EDGE_ID).remove();
    cy.$id(DRAFT_TARGET_NODE_ID).remove();
  });
  activeDraftRelation = null;
}

function renderGraph({ fit = false } = {}) {
  if (!cy) return;
  // 样式只随主题变化重算（refreshGraphTheme），数据/交互重建不重复 getComputedStyle
  const previousPositions = nodePositions();
  const graph = topology.value;
  const elements = [
    ...graph.nodes.map((node) => toCyNode(node, previousPositions)),
    ...graph.edges.map(toCyEdge),
  ];

  cy.batch(() => {
    cy.elements().remove();
    cy.add(elements);
  });

  if (fit) {
    runStructuredLayout({ fit: true, animate: false });
  }
}

function refreshGraphTheme() {
  if (!cy) return;
  cy.style(graphStyles()).update();
  // 主题色进入 SVG data URI，缓存随主题失效后统一重生成
  cardSvgCache.clear();
  cy.batch(() => {
    cy.nodes().forEach((node) => {
      node.data("cardSvg", nodeCardSvg(node.data()));
    });
  });
}

function scheduleGraphThemeRefresh() {
  if (themeRefreshFrame) window.cancelAnimationFrame(themeRefreshFrame);
  window.clearTimeout(themeRefreshTimer);
  themeRefreshFrame = window.requestAnimationFrame(() => {
    themeRefreshFrame = window.requestAnimationFrame(() => {
      themeRefreshFrame = 0;
      refreshGraphTheme();
    });
  });
  themeRefreshTimer = window.setTimeout(refreshGraphTheme, themeTransitionDurationMs() + 40);
}

function themeTransitionDurationMs() {
  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue("--theme-transition-duration")
    .trim();
  const match = raw.match(/^([\d.]+)(ms|s)$/);
  if (!match) return 280;
  const value = Number(match[1]);
  return match[2] === "s" ? value * 1000 : value;
}

function startThemeObserver() {
  if (themeObserver || typeof MutationObserver === "undefined") return;
  themeObserver = new MutationObserver(scheduleGraphThemeRefresh);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "data-theme", "style"],
  });
}

function scheduleRenderGraph({ fit = false } = {}) {
  renderNeedsFit = renderNeedsFit || fit;
  if (renderFrame) window.cancelAnimationFrame(renderFrame);
  renderFrame = window.requestAnimationFrame(() => {
    renderFrame = 0;
    const shouldFit = renderNeedsFit;
    renderNeedsFit = false;
    renderGraph({ fit: shouldFit });
  });
}

async function refreshGraph() {
  disableRelationDrawMode();
  await loadGraphState();
  renderGraph({ fit: true });
}

function nodePositions() {
  if (!cy) return new Map();
  const positions = new Map();
  cy.nodes().forEach((node) => {
    positions.set(node.id(), node.position());
  });
  return positions;
}

// 节点卡片 SVG 只依赖 properties 与主题；拓扑重建时按指纹复用，主题变化时整体失效
let cardSvgCache = new Map();

function nodeCardSvgCached(id, properties) {
  const fingerprint = JSON.stringify([
    properties.title,
    properties.subtitle,
    properties.meta,
    properties.badge,
    properties.kind,
    properties.category,
    properties.protocol,
  ]);
  const cached = cardSvgCache.get(id);
  if (cached?.fingerprint === fingerprint) return cached.uri;
  const uri = nodeCardSvg(properties);
  cardSvgCache.set(id, { fingerprint, uri });
  return uri;
}

function toCyNode(node, previousPositions) {
  const position = previousPositions.get(node.id) || { x: node.x, y: node.y };
  return {
    group: "nodes",
    data: {
      id: node.id,
      label: "",
      cardSvg: nodeCardSvgCached(node.id, node.properties),
      ...node.properties,
    },
    position,
  };
}

function toCyEdge(edge) {
  return {
    group: "edges",
    data: {
      id: edge.id,
      source: edge.sourceNodeId,
      target: edge.targetNodeId,
      label: edge.properties?.label || edge.text,
      text: edge.text,
      ...edge.properties,
    },
  };
}

function cssVar(name) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return toCytoscapeColor(raw) || name;
}

function nodeCardSvg(properties) {
  const palette = nodePalette(properties);
  const title = truncateText(properties.title || "-", 23);
  const subtitle = truncateText(nodeSubtitleText(properties), 34);
  const badge = truncateText(nodeBadgeText(properties), 12);
  const iconBox = properties.category === "credential" ? 32 : 38;
  const iconSize = properties.category === "credential" ? 15 : 18;
  const iconX = 16;
  const iconY = (NODE_SIZE.height - iconBox) / 2;
  const textX = iconX + iconBox + 12;
  const icon = nodeIconSvg(
    properties,
    palette.iconText,
    iconX + (iconBox - iconSize) / 2,
    iconY + (iconBox - iconSize) / 2,
    iconSize,
  );
  const hasBadge = Boolean(badge);
  const badgeWidth = hasBadge ? Math.max(42, estimateSvgTextWidth(badge, 10) + 16) : 0;
  const subtitleMaxWidth = NODE_SIZE.width - textX - (hasBadge ? badgeWidth + 28 : 16);
  const subtitleWidth = Math.min(estimateSvgTextWidth(subtitle, 11), subtitleMaxWidth);
  const badgeMarkup = hasBadge
    ? `<rect x="${NODE_SIZE.width - badgeWidth - 16}" y="47" width="${badgeWidth}" height="18" rx="4" fill="${palette.badgeBg}"/>
  <text x="${NODE_SIZE.width - badgeWidth / 2 - 16}" y="60" fill="${palette.badgeText}" text-anchor="middle" font-family="Ubuntu, Cantarell, Noto Sans, Inter, Segoe UI, Microsoft YaHei, sans-serif" font-size="10" font-weight="700">${escapeXml(badge)}</text>`
    : "";
  const svg = `
<svg xmlns="http://www.w3.org/2000/svg" width="${NODE_SIZE.width}" height="${NODE_SIZE.height}" viewBox="0 0 ${NODE_SIZE.width} ${NODE_SIZE.height}">
  <rect x="0.75" y="0.75" width="${NODE_SIZE.width - 1.5}" height="${NODE_SIZE.height - 1.5}" rx="8" fill="${palette.card}" stroke="${palette.border}" stroke-width="1.5"/>
  <rect x="${iconX}" y="${iconY}" width="${iconBox}" height="${iconBox}" rx="8" fill="${palette.iconBg}"/>
  ${icon}
  <text x="${textX}" y="35" fill="${palette.title}" font-family="Ubuntu, Cantarell, Noto Sans, Inter, Segoe UI, Microsoft YaHei, sans-serif" font-size="13" font-weight="600">${escapeXml(title)}</text>
  <text x="${textX}" y="58" fill="${palette.subtle}" font-family="Ubuntu, Cantarell, Noto Sans, Inter, Segoe UI, Microsoft YaHei, sans-serif" font-size="11" font-weight="500" textLength="${subtitleWidth}" lengthAdjust="spacingAndGlyphs">${escapeXml(subtitle)}</text>
  ${badgeMarkup}
</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

function nodePalette(properties) {
  const base = {
    card: cssVar("--bg-secondary"),
    border: cssVar("--border-light"),
    title: cssVar("--text-primary"),
    subtle: cssVar("--text-secondary"),
    comment: cssVar("--text-tertiary"),
  };
  if (properties.kind === "connection") {
    const protocol = properties.protocol;
    const accent = isTelnetProtocol(protocol)
      ? toCytoscapeColor("light-dark(oklch(64% 0.12 48deg), oklch(79% 0.1 55deg))")
      : isSerialProtocol(protocol)
        ? cssVar("--success")
        : cssVar("--accent");
    const accentBg = isTelnetProtocol(protocol)
      ? toCytoscapeColor("light-dark(oklch(95% 0.03 55deg), oklch(31% 0.038 48deg))")
      : isSerialProtocol(protocol)
        ? cssVar("--success-bg")
        : cssVar("--accent-light");
    return {
      ...base,
      iconBg: accentBg,
      iconText: accent,
      badgeBg: accentBg,
      badgeText: accent,
    };
  }
  const tone = properties.kind === "password-credential" ? cssVar("--success") : cssVar("--accent");
  const toneBg =
    properties.kind === "password-credential" ? cssVar("--success-bg") : cssVar("--accent-light");
  return {
    ...base,
    iconBg: toneBg,
    iconText: tone,
    badgeBg: toneBg,
    badgeText: tone,
  };
}

function nodeIconSvg(properties, color, x, y, size) {
  if (properties.category === "credential") {
    if (properties.kind === "password-credential") {
      return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>`;
    }
    return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m21 2-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.78 7.78 5.5 5.5 0 0 1 7.78-7.78Zm0 0L15.5 7.5m0 0 3 3L22 7l-3-3m-3.5 3.5L19 4"/></svg>`;
  }
  if (isSerialProtocol(properties.protocol)) {
    return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17 19a1 1 0 0 1-1-1v-2a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2a1 1 0 0 1-1 1z"/><path d="M17 21v-2"/><path d="M19 14V6.5a1 1 0 0 0-7 0v11a1 1 0 0 1-7 0V10"/><path d="M21 21v-2"/><path d="M3 5V3"/><path d="M4 10a2 2 0 0 1-2-2V6a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2a2 2 0 0 1-2 2z"/><path d="M7 5V3"/></svg>`;
  }
  if (isTelnetProtocol(properties.protocol)) {
    return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21.54 15H17a2 2 0 0 0-2 2v4.54"/><path d="M7 3.34V5a3 3 0 0 0 3 3a2 2 0 0 1 2 2c0 1.1.9 2 2 2a2 2 0 0 0 2-2c0-1.1.9-2 2-2h3.17"/><path d="M11 21.95V18a2 2 0 0 0-2-2a2 2 0 0 1-2-2v-1a2 2 0 0 0-2-2H2.05"/><circle cx="12" cy="12" r="10"/></svg>`;
  }
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="8" x="2" y="2" rx="2" ry="2"/><rect width="20" height="8" x="2" y="14" rx="2" ry="2"/><line x1="6" x2="6.01" y1="6" y2="6"/><line x1="6" x2="6.01" y1="18" y2="18"/></svg>`;
}

function nodeBadgeText(properties) {
  if (properties.category === "credential") return properties.meta || "";
  return "";
}

function nodeSubtitleText(properties) {
  if (properties.category === "credential") return properties.subtitle || "";
  const protocol = properties.badge ? `${properties.badge} ` : "";
  return `${protocol}${properties.subtitle || ""}`;
}

function truncateText(value, maxLength) {
  const text = String(value || "");
  return text.length > maxLength ? `${text.slice(0, maxLength - 1)}…` : text;
}

function estimateSvgTextWidth(value, fontSize) {
  return Array.from(String(value || "")).reduce((width, char) => {
    if (/[\u1100-\u11ff\u2e80-\u9fff\uf900-\ufaff\uff00-\uffef]/u.test(char)) {
      return width + fontSize;
    }
    return width + fontSize * 0.58;
  }, 0);
}

function escapeXml(value) {
  return String(value).replace(
    /[&<>"']/g,
    (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&apos;" })[char],
  );
}

function toCytoscapeColor(value) {
  if (!value) return "";
  const normalized = selectResolvedColor(value.trim());
  if (normalized.startsWith("oklch(")) return oklchToHex(normalized);
  if (normalized.startsWith("rgb") || normalized.startsWith("#")) return normalized;
  return normalized;
}

function selectResolvedColor(value) {
  if (!value.startsWith("light-dark(")) return value;
  const body = value.slice("light-dark(".length, -1);
  const options = splitColorArguments(body);
  const explicitTheme = document.documentElement.dataset.theme;
  const useDark = explicitTheme ? explicitTheme === "dark" : resolvedTheme.value === "dark";
  return options[useDark ? 1 : 0]?.trim() || options[0]?.trim() || value;
}

function splitColorArguments(value) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (char === "," && depth === 0) {
      parts.push(value.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(value.slice(start));
  return parts;
}

function oklchToHex(value) {
  const match = value.match(
    /oklch\(\s*([\d.]+)%?\s+([\d.]+)\s+([\d.]+)(?:deg)?(?:\s*\/\s*([\d.]+)%?)?\s*\)/i,
  );
  if (!match) return value;
  const lightness = Number(match[1]) > 1 ? Number(match[1]) / 100 : Number(match[1]);
  const chroma = Number(match[2]);
  const hue = (Number(match[3]) * Math.PI) / 180;
  const a = chroma * Math.cos(hue);
  const b = chroma * Math.sin(hue);
  const lPrime = lightness + 0.3963377774 * a + 0.2158037573 * b;
  const mPrime = lightness - 0.1055613458 * a - 0.0638541728 * b;
  const sPrime = lightness - 0.0894841775 * a - 1.291485548 * b;
  const l = lPrime ** 3;
  const m = mPrime ** 3;
  const s = sPrime ** 3;
  return rgbToHex(
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  );
}

function rgbToHex(red, green, blue) {
  return `#${[red, green, blue]
    .map((channel) => {
      const linear = channel <= 0.0031308 ? 12.92 * channel : 1.055 * channel ** (1 / 2.4) - 0.055;
      return Math.round(Math.min(1, Math.max(0, linear)) * 255)
        .toString(16)
        .padStart(2, "0");
    })
    .join("")}`;
}

function buildCredentialTopology() {
  const nodes = [];
  const edges = [];
  const columns = { connection: -260, credential: 280 };
  const counters = { connection: 0, credential: 0 };
  const spacing = { connection: 122, credential: 112 };
  const usedCredentialIds = new Set();

  for (const connection of graphConnections.value) {
    nodes.push(
      createNode({
        id: connectionNodeId(connection.id),
        kind: "connection",
        category: "resource",
        entityId: connection.id,
        title: connection.name,
        subtitle: connectionEndpointLabel(connection),
        meta: t("relationshipGraph.nodeTypes.connection"),
        badge: (connection.protocol || "ssh").toUpperCase(),
        protocol: connection.protocol || "ssh",
        x: columns.connection,
        y: nextY(counters, spacing, "connection"),
      }),
    );
    if (isCredentialConnection(connection) && connection.savedCredentialId) {
      usedCredentialIds.add(connection.savedCredentialId);
      edges.push(
        createEdge({
          id: credentialEdgeId(connection.id, connection.savedCredentialId),
          sourceNodeId: connectionNodeId(connection.id),
          targetNodeId: credentialNodeId(connection.savedCredentialId),
          label: t("relationshipGraph.edges.usesCredential"),
          relation: "credential",
          connectionId: connection.id,
          credentialId: connection.savedCredentialId,
          sourceName: connection.name,
          targetName: credentialName(connection.savedCredentialId),
        }),
      );
    }
  }

  for (const credential of credentials.value) {
    nodes.push(
      createNode({
        id: credentialNodeId(credential.id),
        kind: credential.credType === "key" ? "key-credential" : "password-credential",
        category: "credential",
        entityId: credential.id,
        title: credential.name,
        subtitle: t(`credentials.credTypes.${credential.credType}`),
        meta: t(`credentials.credTypes.${credential.credType}`),
        badge: usedCredentialIds.has(credential.id)
          ? t("relationshipGraph.badges.used")
          : t("relationshipGraph.badges.unused"),
        x: columns.credential,
        y: nextY(counters, spacing, "credential"),
      }),
    );
  }

  return filterTopology(nodes, edges);
}

function filterTopology(nodes, edges) {
  const dedupedNodes = dedupeNodes(nodes);
  const dedupedEdges = dedupeEdges(edges);
  const visibleNodes = dedupedNodes;
  const visibleNodeIds = new Set(visibleNodes.map((node) => node.id));
  const visibleEdges = dedupedEdges.filter(
    (edge) => visibleNodeIds.has(edge.sourceNodeId) && visibleNodeIds.has(edge.targetNodeId),
  );
  return { nodes: visibleNodes, edges: visibleEdges };
}

function requestCreateRelation(edge) {
  const relation = inferRelation(edge, "create");
  if (!relation) {
    showToast({ type: "error", title: t("relationshipGraph.toast.invalidRelation") });
    return;
  }
  // 仅弹确认框，图数据未变，不触发重建
  pendingRelation.value = relation;
}

function requestRemoveRelation(edge) {
  const relation = inferRelation(edge, "remove");
  if (!relation || !relation.persisted) return;
  pendingRelation.value = relation;
}

function requestRemoveSelectedRelations(relations = selectedRemovableRelations()) {
  if (!relations.length) return;
  pendingRelation.value = {
    action: "bulk-remove",
    view: "credential",
    persisted: true,
    relations,
  };
}

function requestDeleteCredentialNode(data) {
  if (data?.category !== "credential" || !data.entityId) return;
  requestCredentialDelete({
    id: data.entityId,
    name: data.title || data.entityId,
  });
}

function requestDeleteSelectedCredentialNodes(credentialsToDelete = selectedCredentialNodes()) {
  const normalized = credentialsToDelete
    .map((node) => node.data())
    .filter((data) => data?.category === "credential" && data.entityId)
    .map((data) => ({
      id: data.entityId,
      name: data.title || data.entityId,
      usages: credentialUsages(data.entityId),
    }));
  if (!normalized.length) return;
  cancelExitTeardown();
  pendingBulkCredentialDelete.value = {
    credentials: normalized,
    usageCount: normalized.reduce((count, credential) => count + credential.usages.length, 0),
  };
  bulkCredentialDeleteOpen.value = true;
}

function contextMenuItem(id, label, icon, enabled, action, options = {}) {
  return { id, label, icon, enabled, action, ...options };
}

function buildNodeMenuItems(node) {
  const data = node.data();
  const items = [
    contextMenuItem("relationship-edit", t("relationshipGraph.menu.edit"), "edit", true, () =>
      openEditorForNode(data),
    ),
    contextMenuItem(
      "relationship-focus-related",
      t("relationshipGraph.menu.focusRelated"),
      "search",
      true,
      () => focusRelated(node.id()),
    ),
    contextMenuItem(
      "relationship-quick-add",
      t("relationshipGraph.menu.quickAddRelation"),
      "redo",
      canStartRelationFromNode(data),
      () => startRelationFromNode(node.id()),
    ),
  ];
  if (data.category === "credential") {
    items.push(
      contextMenuItem(
        "relationship-delete-credential",
        t("relationshipGraph.menu.deleteCredential"),
        "delete",
        true,
        () => requestDeleteCredentialNode(data),
        { tone: "danger" },
      ),
    );
  }
  return items;
}

function canStartRelationFromNode(nodeData) {
  const category = nodeData?.category;
  return (
    category === "credential" ||
    (category === "resource" && isCredentialConnectionId(nodeData?.entityId))
  );
}

function startRelationFromNode(nodeId) {
  const node = cy?.$id(nodeId);
  if (!node?.length) return;
  setRelationDrawMode(true);
  node.select();
  showToast({
    type: "success",
    title: t("relationshipGraph.menu.quickAddRelation"),
    message: t("relationshipGraph.toast.openRelationEditor"),
  });
}

function toggleRelationDrawMode() {
  setRelationDrawMode(!relationDrawMode.value);
}

function disableRelationDrawMode() {
  setRelationDrawMode(false);
}

function setRelationDrawMode(enabled) {
  relationDrawMode.value = enabled;
  cy?.autoungrabify(enabled);
  if (!enabled) cancelDraftRelation();
}

function buildEdgeMenuItems(edge) {
  const removable = Boolean(inferRelation(toTopologyEdgeFromCy(edge, true), "remove")?.persisted);
  return [
    contextMenuItem(
      "relationship-remove-relation",
      t("relationshipGraph.menu.removeRelation"),
      "delete",
      removable,
      () => requestRemoveRelation(toTopologyEdgeFromCy(edge, true)),
      { tone: "danger" },
    ),
  ];
}

function buildSelectionMenuItems() {
  const selection = selectedElements();
  if (!selection.length) return [];
  const selectedCredentials = selectedCredentialNodes();
  const removableRelations = selectedRemovableRelations();
  return [
    contextMenuItem(
      "relationship-selected-remove-relations",
      t("relationshipGraph.menu.removeSelectedRelations", {
        count: removableRelations.length,
      }),
      "delete",
      removableRelations.length > 0,
      () => requestRemoveSelectedRelations(removableRelations),
      { tone: "danger" },
    ),
    contextMenuItem(
      "relationship-selected-delete-credentials",
      t("relationshipGraph.menu.deleteSelectedCredentials", {
        count: selectedCredentials.length,
      }),
      "delete",
      selectedCredentials.length > 0,
      () => requestDeleteSelectedCredentialNodes(selectedCredentials),
      { tone: "danger" },
    ),
    { type: "separator" },
    contextMenuItem(
      "relationship-clear-selection",
      t("relationshipGraph.menu.clearSelection"),
      "clear",
      true,
      () => cy?.elements().unselect(),
    ),
  ];
}

function buildGraphMenuItems() {
  return [
    contextMenuItem(
      "relationship-refresh",
      t("relationshipGraph.menu.refresh"),
      "refresh",
      true,
      refreshGraph,
    ),
  ];
}

async function provideContextMenu(event) {
  if (!cy) return;
  if (isEditableContextTarget(event.target)) return;
  if (!event.target?.closest?.(".relationship-canvas")) return;
  const element = elementAtPoint(event);
  const hasSelection = selectedElements().length > 1;
  const items =
    hasSelection && (!element || element.selected?.())
      ? buildSelectionMenuItems()
      : element?.isNode?.()
        ? buildNodeMenuItems(element)
        : element?.isEdge?.()
          ? buildEdgeMenuItems(element)
          : buildGraphMenuItems();
  await openContextMenu(event, {
    suppressDefaultEditItems: true,
    items,
  });
}

function elementAtPoint(nativeEvent) {
  const rect = graphEl.value?.getBoundingClientRect();
  if (!rect) return null;
  const point = {
    x: nativeEvent.clientX - rect.left,
    y: nativeEvent.clientY - rect.top,
  };
  const hit = (element) => {
    const box = element.renderedBoundingBox({ includeLabels: true, includeOverlays: false });
    return point.x >= box.x1 && point.x <= box.x2 && point.y >= box.y1 && point.y <= box.y2;
  };
  return cy.nodes().filter(hit)[0] || cy.edges().filter(hit)[0] || null;
}

function selectedElements() {
  if (!cy) return [];
  return cy.elements(":selected").not(".relationship-draft-target, .relationship-draft-edge");
}

function selectedCredentialNodes() {
  return selectedElements()
    .filter("node")
    .filter((node) => node.data("category") === "credential" && node.data("entityId"));
}

function selectedRemovableRelations() {
  if (!cy) return [];
  const selected = selectedElements();
  const selectedNodeIds = new Set(selected.filter("node").map((node) => node.id()));
  const selectedEdgeIds = new Set(selected.filter("edge").map((edge) => edge.id()));
  const seen = new Set();
  const relations = [];
  cy.edges().forEach((edge) => {
    if (edge.hasClass("relationship-draft-edge")) return;
    if (seen.has(edge.id())) return;
    const edgeSelected = selectedEdgeIds.has(edge.id());
    const endpointsSelected =
      selectedNodeIds.has(edge.source().id()) && selectedNodeIds.has(edge.target().id());
    if (!edgeSelected && !endpointsSelected) return;
    const relation = inferRelation(toTopologyEdgeFromCy(edge, true), "remove");
    if (!relation?.persisted) return;
    seen.add(edge.id());
    relations.push(relation);
  });
  return relations;
}

function isEditableContextTarget(target) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target?.isContentEditable
  );
}

function edgeFromCyNodes(sourceNodeId, targetNodeId) {
  return {
    id: `draft-${sourceNodeId}-${targetNodeId}`,
    sourceNodeId,
    targetNodeId,
    text: "",
    properties: { persisted: false },
  };
}

function toTopologyEdgeFromCy(edge, persisted) {
  return {
    id: edge.id(),
    sourceNodeId: edge.source().id(),
    targetNodeId: edge.target().id(),
    text: edge.data("label"),
    properties: {
      ...edge.data(),
      persisted,
      label: edge.data("label"),
    },
  };
}

function inferRelation(edge, action) {
  let source = topologyNodeMap.value.get(edge.sourceNodeId);
  let target = topologyNodeMap.value.get(edge.targetNodeId);
  const persisted = Boolean(edge.properties?.persisted);
  if (!source || !target) return null;

  if (source.category === "credential" && target.category === "resource") {
    [source, target] = [target, source];
  }
  if (
    source.category === "resource" &&
    target.category === "credential" &&
    isCredentialConnectionId(source.entityId) &&
    target.entityId
  ) {
    return {
      action,
      view: "credential",
      persisted,
      connectionId: source.entityId,
      credentialId: target.entityId,
      sourceName: source.title,
      targetName: target.title,
    };
  }
  return null;
}

async function confirmRelationChange() {
  const relation = pendingRelation.value;
  if (!relation || relationBusy.value) return;
  relationBusy.value = true;
  try {
    if (relation.action === "bulk-remove") {
      for (const item of relation.relations) {
        if (item.view === "credential") {
          await clearConnectionSavedCredential(item.connectionId);
        }
      }
    } else if (relation.view === "credential") {
      if (relation.action === "create") {
        await setConnectionSavedCredential(relation.connectionId, relation.credentialId);
      } else {
        await clearConnectionSavedCredential(relation.connectionId);
      }
    }
    pendingRelation.value = null;
    await refreshGraph();
    emit("state-changed");
    showToast({ type: "success", title: t("relationshipGraph.toast.relationUpdated") });
  } catch (error) {
    showToast({
      type: "error",
      title: t("relationshipGraph.toast.relationUpdateFailed"),
      message: String(error),
    });
    await refreshGraph();
  } finally {
    relationBusy.value = false;
  }
}

function cancelRelationChange() {
  if (relationBusy.value) return;
  pendingRelation.value = null;
}

async function confirmBulkCredentialDelete() {
  const pending = pendingBulkCredentialDelete.value;
  if (!pending || credentialDeleteBusy.value) return;
  credentialDeleteBusy.value = true;
  try {
    for (const credential of pending.credentials) {
      if (credential.usages.length) {
        await clearCredentialReferences(credential.id);
      }
      await deleteCredential(credential.id);
    }
    bulkCredentialDeleteOpen.value = false;
    // 退出动画期间保留 pending 数据，slot 里的删除列表不会先于弹壳消失
    scheduleExitTeardown(() => {
      pendingBulkCredentialDelete.value = null;
    });
    await refreshGraph();
    emit("state-changed");
    showToast({ type: "success", title: t("notifications.credentialDeleted") });
  } catch (error) {
    showToast({
      type: "error",
      title: t("notifications.credentialDeleteFailed"),
      message: String(error),
    });
    await refreshGraph();
  } finally {
    credentialDeleteBusy.value = false;
  }
}

function cancelBulkCredentialDelete() {
  if (credentialDeleteBusy.value) return;
  bulkCredentialDeleteOpen.value = false;
  pendingBulkCredentialDelete.value = null;
}

function credentialUsages(credentialId) {
  return normalizeCredentialUsages(credentialUsagesList.value, credentialId);
}

function createNode({
  id,
  kind,
  category,
  entityId,
  title,
  subtitle,
  meta,
  badge,
  protocol,
  x,
  y,
}) {
  return {
    id,
    x,
    y,
    properties: {
      id,
      kind,
      category,
      entityId,
      title,
      subtitle,
      meta,
      badge,
      protocol,
    },
  };
}

function createEdge({
  id,
  sourceNodeId,
  targetNodeId,
  label,
  relation,
  connectionId,
  credentialId,
  sourceName,
  targetName,
}) {
  return {
    id,
    sourceNodeId,
    targetNodeId,
    text: label,
    properties: {
      id,
      label,
      relation,
      persisted: true,
      connectionId,
      credentialId,
      sourceName,
      targetName,
    },
  };
}

function dedupeNodes(nodes) {
  const seen = new Set();
  return nodes.filter((node) => {
    if (seen.has(node.id)) return false;
    seen.add(node.id);
    return true;
  });
}

function dedupeEdges(edges) {
  const seen = new Set();
  return edges.filter((edge) => {
    const key = `${edge.sourceNodeId}:${edge.targetNodeId}:${edge.properties?.relation}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function nextY(counters, spacing, key) {
  const y = 24 + counters[key] * spacing[key];
  counters[key] += 1;
  return y;
}

function connectionNodeId(id) {
  return `connection-${id}`;
}

function credentialNodeId(id) {
  return `credential-${id}`;
}

function credentialEdgeId(connectionId, credentialId) {
  return `credential-edge-${connectionId}-${credentialId}`;
}

function isCredentialConnection(connection) {
  return supportsSavedCredential(connection?.protocol || "ssh");
}

function isCredentialConnectionId(connectionId) {
  return graphConnectionIds.value.has(connectionId);
}

function credentialName(id) {
  return credentials.value.find((credential) => credential.id === id)?.name || id;
}

function zoomGraph(zoomIn) {
  if (!cy) return;
  const renderedPosition = {
    x: graphEl.value.clientWidth / 2,
    y: graphEl.value.clientHeight / 2,
  };
  cy.zoom({
    level: cy.zoom() * (zoomIn ? 1.18 : 0.84),
    renderedPosition,
  });
}

function runStructuredLayout({ fit = false, animate = true } = {}) {
  if (!cy || !cy.nodes().length) return;
  const graph = topology.value;
  const positions = structuredPositions(graph);
  const elements = layoutElements();
  cy.batch(() => {
    for (const [id, position] of positions) {
      const node = cy.$id(id);
      if (animate) {
        node.animate({ position }, { duration: 180, easing: "ease-out" });
      } else {
        node.position(position);
      }
    }
  });
  if (fit) {
    if (animate) {
      layoutFitTimer = window.setTimeout(() => {
        layoutFitTimer = 0;
        if (cy && elements.length)
          cy.animate({ fit: { eles: elements, padding: 88 } }, { duration: 140 });
      }, 190);
    } else if (elements.length) {
      cy.fit(elements, 88);
    }
  }
}

function structuredPositions(graph) {
  const laneDefinitions = [
    { key: "resource", x: -260 },
    { key: "credential", x: 300 },
  ];
  const lanes = laneDefinitions.map((lane) => ({ ...lane, nodes: [] }));
  const byLane = new Map(lanes.map((lane) => [lane.key, lane]));
  for (const node of graph.nodes) {
    const category = node.properties?.category;
    const laneKey = byLane.has(category) ? category : lanes[0].key;
    byLane.get(laneKey).nodes.push(node);
  }
  orderStructuredLanes(lanes, graph.edges);
  const positions = new Map();
  for (const lane of lanes) {
    const laneNodes = lane.nodes;
    const gap = NODE_SIZE.height + 56;
    const startY = -((laneNodes.length - 1) * gap) / 2;
    laneNodes.forEach((node, index) => {
      positions.set(node.id, { x: lane.x, y: startY + index * gap });
    });
  }
  return positions;
}

function orderStructuredLanes(lanes, edges) {
  const laneByKey = new Map(lanes.map((lane) => [lane.key, lane]));
  for (const lane of lanes) {
    lane.nodes.sort(compareNodeTitles);
  }
  const primaryLane = laneByKey.get("resource");
  const secondaryLane = laneByKey.get("credential");
  if (!primaryLane || !secondaryLane) return;
  const primaryOrder = new Map(primaryLane.nodes.map((node, index) => [node.id, index]));
  const relatedScores = relatedOrderScores(edges, primaryOrder);
  secondaryLane.nodes.sort((left, right) => {
    const leftScore = relatedScores.get(left.id) ?? Number.MAX_SAFE_INTEGER;
    const rightScore = relatedScores.get(right.id) ?? Number.MAX_SAFE_INTEGER;
    if (leftScore !== rightScore) return leftScore - rightScore;
    return compareNodeTitles(left, right);
  });
}

function relatedOrderScores(edges, orderMap) {
  const totals = new Map();
  for (const edge of edges) {
    const sourceScore = orderMap.get(edge.sourceNodeId);
    const targetScore = orderMap.get(edge.targetNodeId);
    if (sourceScore !== undefined && targetScore === undefined) {
      addRelatedScore(totals, edge.targetNodeId, sourceScore);
    } else if (targetScore !== undefined && sourceScore === undefined) {
      addRelatedScore(totals, edge.sourceNodeId, targetScore);
    }
  }
  return new Map([...totals].map(([nodeId, total]) => [nodeId, total.sum / total.count]));
}

function addRelatedScore(totals, nodeId, score) {
  const total = totals.get(nodeId) || { sum: 0, count: 0 };
  total.sum += score;
  total.count += 1;
  totals.set(nodeId, total);
}

function compareNodeTitles(left, right) {
  return String(left.properties?.title || "").localeCompare(String(right.properties?.title || ""));
}

function layoutElements() {
  return cy.elements().not(".relationship-draft-target, .relationship-draft-edge");
}

function focusRelated(nodeId) {
  if (!cy) return;
  const node = cy.$id(nodeId);
  if (!node.length) return;
  const neighborhood = node.closedNeighborhood();
  cy.elements().unselect();
  neighborhood.select();
  cy.animate({ fit: { eles: neighborhood, padding: 72 }, duration: 140 });
}

function openEditorForNode(properties = {}) {
  if (!properties.entityId) return;
  router.push({
    name: properties.category === "credential" ? "keys" : "sessions",
    query: { edit: properties.entityId },
  });
}

function openNodeTarget(properties = {}) {
  if (!properties.entityId) return;
  if (properties.category === "credential") {
    openEditorForNode(properties);
    return;
  }
  if (properties.category === "resource" && workspace.connectTo(properties.entityId)) {
    router.push({ name: "workspace" });
  }
}

watch(topology, async () => {
  await nextTick();
  if (!cy) {
    initGraph();
    return;
  }
  scheduleRenderGraph();
});

watch(resolvedTheme, async () => {
  await nextTick();
  scheduleGraphThemeRefresh();
});

defineExpose({
  refreshGraph,
  runStructuredLayout,
  zoomGraph,
});
</script>

<template>
  <div
    class="relationship-root"
    :class="{ 'relationship-root-embedded': props.embedded }"
    @contextmenu="provideContextMenu"
  >
    <div
      v-if="!props.embedded"
      class="relationship-toolbar"
    >
      <div class="relationship-title-group">
        <GitBranch
          :size="18"
          stroke-width="1.7"
          class="text-accent"
        />
        <div>
          <h2 class="ui-page-title">
            {{ t("relationshipGraph.title") }}
          </h2>
          <p class="ui-page-desc">
            {{ t("relationshipGraph.description") }}
          </p>
        </div>
      </div>

      <div class="relationship-stats">
        <span>{{ t("relationshipGraph.views.credential") }}</span>
        <span>{{ t("relationshipGraph.stats.connections", { count: stats.connections }) }}</span>
        <span>{{ t("relationshipGraph.stats.credentials", { count: stats.credentials }) }}</span>
        <span>{{ t("relationshipGraph.stats.edges", { count: stats.edges }) }}</span>
      </div>
    </div>

    <div
      v-if="graphError"
      class="relationship-error"
    >
      {{ graphError }}
    </div>

    <div class="relationship-surface">
      <div class="relationship-graph-tools">
        <AppTooltip
          :content="t('relationshipGraph.actions.zoomOut')"
          side="bottom"
        >
          <button
            type="button"
            class="relationship-tool-button"
            :aria-label="t('relationshipGraph.actions.zoomOut')"
            @click="zoomGraph(false)"
          >
            <Minus
              :size="14"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="t('relationshipGraph.actions.zoomIn')"
          side="bottom"
        >
          <button
            type="button"
            class="relationship-tool-button"
            :aria-label="t('relationshipGraph.actions.zoomIn')"
            @click="zoomGraph(true)"
          >
            <Plus
              :size="14"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="t('relationshipGraph.actions.refreshAndReset')"
          side="bottom"
        >
          <button
            type="button"
            class="relationship-tool-button"
            :aria-label="t('relationshipGraph.actions.refreshAndReset')"
            :disabled="loading"
            @click="refreshGraph"
          >
            <RefreshCw
              :size="14"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="t('relationshipGraph.actions.relationMode')"
          side="bottom"
        >
          <button
            type="button"
            class="relationship-tool-button"
            :class="{ 'is-active': relationDrawMode }"
            :aria-label="t('relationshipGraph.actions.relationMode')"
            @click="toggleRelationDrawMode"
          >
            <Link2
              :size="14"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="t('relationshipGraph.actions.stabilize')"
          side="bottom"
        >
          <button
            type="button"
            class="relationship-tool-button"
            :aria-label="t('relationshipGraph.actions.stabilize')"
            @click="runStructuredLayout({ fit: true })"
          >
            <SlidersHorizontal
              :size="14"
              stroke-width="1.8"
            />
          </button>
        </AppTooltip>
      </div>

      <div class="relationship-canvas-shell">
        <div
          ref="graphEl"
          class="relationship-canvas"
        />
      </div>

      <div
        v-if="!topology.nodes.length"
        class="relationship-empty"
      >
        <GitBranch
          :size="34"
          stroke-width="1.3"
        />
        <span>{{ t("relationshipGraph.empty") }}</span>
      </div>
    </div>

    <ConfirmDialog
      :open="Boolean(pendingRelation)"
      :tone="pendingConfirmTone"
      :loading="relationBusy"
      :title="t('relationshipGraph.confirm.title')"
      :description="pendingConfirmText"
      :confirm-text="t('relationshipGraph.confirm.confirm')"
      :confirm-icon="pendingConfirmIcon"
      @update:open="
        (value) => {
          if (!value) cancelRelationChange();
        }
      "
      @confirm="confirmRelationChange"
    />

    <ConfirmDialog
      :open="credentialDeleteOpen"
      tone="danger"
      :loading="credentialDeleteBusy"
      :title="t('relationshipGraph.confirm.credentialDelete.title')"
      :description="pendingCredentialDeleteDescription"
      :confirm-text="t('relationshipGraph.confirm.credentialDelete.confirm')"
      :confirm-icon="Trash2"
      @update:open="
        (value) => {
          if (!value) cancelCredentialDelete();
        }
      "
      @confirm="confirmCredentialDelete"
    >
      <div
        v-if="pendingCredentialDelete?.usages?.length"
        class="relationship-delete-usage-list"
      >
        <span
          v-for="usage in pendingCredentialDelete.usages"
          :key="`${usage.connectionId}:${usage.relation}`"
          class="relationship-delete-usage-item"
          :title="`${usage.connectionName} · ${t(`credentials.relations.${usage.relation}`)}`"
        >
          <span class="relationship-delete-usage-name">{{ usage.connectionName }}</span>
          <span class="relationship-delete-usage-relation">
            {{ t(`credentials.relations.${usage.relation}`) }}
          </span>
        </span>
      </div>
    </ConfirmDialog>

    <ConfirmDialog
      :open="bulkCredentialDeleteOpen"
      tone="danger"
      :loading="credentialDeleteBusy"
      :title="t('relationshipGraph.confirm.credentialDelete.bulkTitle')"
      :description="pendingBulkCredentialDeleteDescription"
      :confirm-text="t('relationshipGraph.confirm.credentialDelete.confirm')"
      :confirm-icon="Trash2"
      @update:open="
        (value) => {
          if (!value) cancelBulkCredentialDelete();
        }
      "
      @confirm="confirmBulkCredentialDelete"
    >
      <div
        v-if="pendingBulkCredentialDelete?.credentials?.length"
        class="relationship-delete-usage-list"
      >
        <span
          v-for="credential in pendingBulkCredentialDelete.credentials"
          :key="credential.id"
          class="relationship-delete-usage-item"
          :title="credential.name"
        >
          <span class="relationship-delete-usage-name">{{ credential.name }}</span>
          <span
            v-if="credential.usages.length"
            class="relationship-delete-usage-relation"
          >
            {{ credential.usages.length }}
          </span>
        </span>
      </div>
    </ConfirmDialog>
  </div>
</template>
