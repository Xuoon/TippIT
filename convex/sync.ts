import { v } from "convex/values";
import { sha256 } from "js-sha256";
import { mutation, query } from "./_generated/server";
import { requireGroup } from "./lib";

const MAX_INLINE_CIPHER = 900 * 1024; // Dokumentlimit ~1 MiB
const MAX_PUSH_BYTES = 8 * 1024 * 1024;
const MAX_ENTRIES_PER_PUSH = 50;
// Ein Eintrag darf knapp 900 KiB groß sein; acht Einträge bleiben inklusive
// Convex-/JSON-Overhead konservativ unter dem Query-Ergebnislimit.
const PULL_PAGE = 8;
const SETTINGS_UUID = "00000000-0000-4000-8000-5e771465e771";

const entryArg = v.object({
  uuid: v.string(),
  kind: v.number(),
  cipher: v.optional(v.bytes()),
  thumbCipher: v.optional(v.bytes()),
  pinned: v.boolean(),
  createdAt: v.number(),
  deleted: v.boolean(),
  lamport: v.number(),
  deviceId: v.string(),
});

interface SyncEntryArg {
  cipher?: ArrayBuffer;
  createdAt: number;
  deleted: boolean;
  deviceId: string;
  kind: number;
  lamport: number;
  pinned: boolean;
  thumbCipher?: ArrayBuffer;
  uuid: string;
}

function hasValidMetadata(entry: SyncEntryArg): boolean {
  const validUuid = entry.uuid.length > 0 && entry.uuid.length <= 128;
  const validDevice = entry.deviceId.length > 0 && entry.deviceId.length <= 128;
  const validKind =
    Number.isSafeInteger(entry.kind) && entry.kind >= 0 && entry.kind <= 3;
  const validLamport =
    Number.isSafeInteger(entry.lamport) && entry.lamport >= 0;
  const validCreatedAt =
    Number.isSafeInteger(entry.createdAt) && entry.createdAt >= 0;
  return (
    validUuid && validDevice && validKind && validLamport && validCreatedAt
  );
}

function validateEntry(entry: SyncEntryArg): number {
  if (!hasValidMetadata(entry)) {
    throw new Error("Ungültige Sync-Metadaten");
  }
  if ((entry.uuid === SETTINGS_UUID) !== (entry.kind === 3)) {
    throw new Error("Ungültiger Settings-Eintrag");
  }
  if (entry.deleted && (entry.cipher || entry.thumbCipher)) {
    throw new Error("Tombstones dürfen keinen Ciphertext enthalten");
  }
  if (!(entry.deleted || entry.cipher)) {
    throw new Error("Aktiver Eintrag benötigt Ciphertext");
  }
  if (entry.thumbCipher && entry.kind !== 1) {
    throw new Error("Thumbnail ist nur für Bilder erlaubt");
  }
  return (entry.cipher?.byteLength ?? 0) + (entry.thumbCipher?.byteLength ?? 0);
}

/** Idempotent: existiert die Gruppe mit gleichem Hash, ist das ok. */
export const createGroup = mutation({
  args: { groupId: v.string(), authKey: v.string() },
  returns: v.object({ created: v.boolean() }),
  handler: async (ctx, args) => {
    const existing = await ctx.db
      .query("groups")
      .withIndex("by_groupId", (q) => q.eq("groupId", args.groupId))
      .unique();
    const hash = sha256(args.authKey);
    if (existing) {
      if (existing.authKeyHash !== hash) {
        throw new Error("Gruppen-ID kollidiert");
      }
      return { created: false };
    }
    await ctx.db.insert("groups", {
      groupId: args.groupId,
      authKeyHash: hash,
      createdAt: Date.now(),
    });
    return { created: true };
  },
});

/** LWW-Upsert pro Eintrag (Vergleich über lamport+deviceId);
 * jeder akzeptierte Eintrag bekommt eine neue serverseitige seq. */
export const push = mutation({
  args: {
    groupId: v.string(),
    authKey: v.string(),
    entries: v.array(entryArg),
  },
  returns: v.object({ accepted: v.number(), maxLamport: v.number() }),
  handler: async (ctx, args) => {
    await requireGroup(ctx, args.groupId, args.authKey);
    if (args.entries.length > MAX_ENTRIES_PER_PUSH) {
      throw new Error("Zu viele Einträge pro Push");
    }
    const pushBytes = args.entries.reduce(
      (sum, entry) =>
        sum +
        (entry.cipher?.byteLength ?? 0) +
        (entry.thumbCipher?.byteLength ?? 0),
      0
    );
    if (pushBytes > MAX_PUSH_BYTES) {
      throw new Error("Push-Batch zu groß");
    }
    const latest = await ctx.db
      .query("entries")
      .withIndex("by_group_seq", (q) => q.eq("groupId", args.groupId))
      .order("desc")
      .first();
    let seq = latest?.seq ?? 0;
    let maxLamport = latest?.lamport ?? 0;
    let accepted = 0;

    for (const entry of args.entries) {
      const entryBytes = validateEntry(entry);
      if (entryBytes > MAX_INLINE_CIPHER) {
        throw new Error(`Eintrag ${entry.uuid} zu groß für Inline-Sync`);
      }
      maxLamport = Math.max(maxLamport, entry.lamport);
      const existing = await ctx.db
        .query("entries")
        .withIndex("by_group_uuid", (q) =>
          q.eq("groupId", args.groupId).eq("uuid", entry.uuid)
        )
        .unique();
      const newer =
        !existing ||
        entry.lamport > existing.lamport ||
        (entry.lamport === existing.lamport &&
          entry.deviceId > existing.deviceId);
      if (!newer) {
        continue;
      }
      accepted++;
      seq++;
      if (existing) {
        // replace statt patch: Tombstones müssen cipher serverseitig wirklich entfernen
        await ctx.db.replace(existing._id, {
          ...entry,
          groupId: args.groupId,
          seq,
        });
      } else {
        await ctx.db.insert("entries", {
          ...entry,
          groupId: args.groupId,
          seq,
        });
      }
    }
    return { accepted, maxLamport };
  },
});

/** Seitenweiser Pull über den seq-Cursor. Eigene Einträge (excludeDevice)
 * werden serverseitig gefiltert, der Cursor (maxSeq) läuft trotzdem weiter —
 * so lädt kein Gerät seine eigenen Push-Echos herunter. */
export const pullSince = query({
  args: {
    groupId: v.string(),
    authKey: v.string(),
    since: v.number(),
    excludeDevice: v.optional(v.string()),
  },
  returns: v.object({
    entries: v.array(entryArg),
    maxSeq: v.number(),
    hasMore: v.boolean(),
  }),
  handler: async (ctx, args) => {
    await requireGroup(ctx, args.groupId, args.authKey);
    const page = await ctx.db
      .query("entries")
      .withIndex("by_group_seq", (q) =>
        q.eq("groupId", args.groupId).gt("seq", args.since)
      )
      .order("asc")
      .take(PULL_PAGE + 1);
    const hasMore = page.length > PULL_PAGE;
    const window = page.slice(0, PULL_PAGE);
    const maxSeq = window.at(-1)?.seq ?? args.since;
    return {
      entries: window
        .filter((e) => e.deviceId !== args.excludeDevice)
        .map(({ _id, _creationTime, groupId, seq, ...e }) => e),
      maxSeq,
      hasMore,
    };
  },
});

/** Gerät der Geräteliste melden bzw. „zuletzt aktiv" auffrischen
 * (bei Sync-Session-Start und danach sparsam periodisch). */
export const announceDevice = mutation({
  args: {
    groupId: v.string(),
    authKey: v.string(),
    deviceId: v.string(),
    name: v.string(),
    platform: v.string(),
  },
  returns: v.null(),
  handler: async (ctx, args) => {
    await requireGroup(ctx, args.groupId, args.authKey);
    const validDevice = args.deviceId.length > 0 && args.deviceId.length <= 128;
    const validMeta = args.name.length <= 64 && args.platform.length <= 16;
    if (!(validDevice && validMeta)) {
      throw new Error("Ungültige Geräte-Metadaten");
    }
    const existing = await ctx.db
      .query("devices")
      .withIndex("by_group_device", (q) =>
        q.eq("groupId", args.groupId).eq("deviceId", args.deviceId)
      )
      .unique();
    if (existing) {
      await ctx.db.patch(existing._id, {
        name: args.name,
        platform: args.platform,
        lastSeenAt: Date.now(),
      });
    } else {
      await ctx.db.insert("devices", {
        groupId: args.groupId,
        deviceId: args.deviceId,
        name: args.name,
        platform: args.platform,
        lastSeenAt: Date.now(),
      });
    }
    return null;
  },
});

/** Geräte der Gruppe für die Anzeige im Sync-Tab. */
export const listDevices = query({
  args: { groupId: v.string(), authKey: v.string() },
  returns: v.array(
    v.object({
      deviceId: v.string(),
      name: v.string(),
      platform: v.string(),
      lastSeenAt: v.number(),
    })
  ),
  handler: async (ctx, args) => {
    await requireGroup(ctx, args.groupId, args.authKey);
    const devices = await ctx.db
      .query("devices")
      .withIndex("by_group_device", (q) => q.eq("groupId", args.groupId))
      .collect();
    return devices.map(({ deviceId, name, platform, lastSeenAt }) => ({
      deviceId,
      name,
      platform,
      lastSeenAt,
    }));
  },
});

/** Billiges Subscription-Target: weckt Clients, wenn es Neues gibt. */
export const latestSeq = query({
  args: { groupId: v.string(), authKey: v.string() },
  returns: v.number(),
  handler: async (ctx, args) => {
    await requireGroup(ctx, args.groupId, args.authKey);
    const latest = await ctx.db
      .query("entries")
      .withIndex("by_group_seq", (q) => q.eq("groupId", args.groupId))
      .order("desc")
      .first();
    return latest?.seq ?? 0;
  },
});
