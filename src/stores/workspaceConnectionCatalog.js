import { computed, shallowRef, triggerRef } from "vue";

export function createWorkspaceConnectionCatalog() {
  const records = shallowRef(new Map());
  const profileOrder = shallowRef([]);
  const externalOrder = shallowRef([]);

  const profileConnections = computed(() => project(profileOrder.value));
  const connections = computed(() => project(profileOrder.value, externalOrder.value));
  const connectionsById = records;

  function project(...orders) {
    const projected = [];
    for (const order of orders) {
      for (const id of order) {
        const connection = records.value.get(id);
        if (connection) projected.push(connection);
      }
    }
    return projected;
  }

  function replaceRecord(connection) {
    records.value.set(connection.id, connection);
    triggerRef(records);
  }

  function patchRecord(id, patch) {
    const current = records.value.get(id);
    if (!current) return false;
    replaceRecord({ ...current, ...patch });
    return true;
  }

  function removeRecord(id) {
    if (!records.value.delete(id)) return;
    triggerRef(records);
  }

  function commitRecords(nextRecords) {
    records.value = nextRecords;
  }

  function setOrder(orderRef, order) {
    orderRef.value = Array.isArray(order) ? [...order] : [];
  }

  function getConnection(id) {
    return records.value.get(id) ?? null;
  }

  function setProfiles(profiles) {
    const nextProfiles = Array.isArray(profiles) ? profiles.filter(Boolean) : [];
    const nextProfileIds = new Set(nextProfiles.map((connection) => connection.id));
    const previousProfileIds = new Set(profileOrder.value);
    const nextRecords = new Map(records.value);

    for (const id of previousProfileIds) {
      if (!nextProfileIds.has(id)) nextRecords.delete(id);
    }
    for (const profile of nextProfiles) {
      nextRecords.set(profile.id, { ...profile, source: "profile", external: false });
    }
    commitRecords(nextRecords);

    setOrder(
      profileOrder,
      nextProfiles.map((connection) => connection.id),
    );
    setOrder(
      externalOrder,
      externalOrder.value.filter((id) => !nextProfileIds.has(id)),
    );
  }

  function reorderProfiles(order) {
    const profileIds = new Set(profileOrder.value);
    setOrder(
      profileOrder,
      order.filter((id) => profileIds.has(id)),
    );
  }

  function removeProfile(id) {
    setOrder(
      profileOrder,
      profileOrder.value.filter((connectionId) => connectionId !== id),
    );
    removeRecord(id);
  }

  function upsertExternal(connection) {
    if (!connection?.id) return false;
    const next = { ...connection, source: "transient", external: true };
    replaceRecord(next);
    if (!externalOrder.value.includes(next.id)) {
      setOrder(externalOrder, [next.id, ...externalOrder.value]);
    }
    return true;
  }

  function removeExternal(id) {
    setOrder(
      externalOrder,
      externalOrder.value.filter((connectionId) => connectionId !== id),
    );
    if (!profileOrder.value.includes(id)) removeRecord(id);
  }

  return {
    connections,
    connectionsById,
    getConnection,
    patchRecord,
    profileConnections,
    removeExternal,
    removeProfile,
    reorderProfiles,
    setProfiles,
    upsertExternal,
  };
}
