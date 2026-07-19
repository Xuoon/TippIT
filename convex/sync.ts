import { v } from "convex/values";
import { sha256 } from "js-sha256";
import { mutation, query } from "./_generated/server";
import { requireGroup } from "./lib";

const MAX_INLINE_CIPHER = 900 * 1024; // Dokumentlimit ~1 MiB
const MAX_ENTRIES_PER_PUSH = 50;
const PULL_PAGE = 200;

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
    const latest = await ctx.db
      .query("entries")
      .withIndex("by_group_seq", (q) => q.eq("groupId", args.groupId))
      .order("desc")
      .first();
    let seq = latest?.seq ?? 0;
    let maxLamport = latest?.lamport ?? 0;
    let accepted = 0;

    for (const entry of args.entries) {
      if (entry.cipher && entry.cipher.byteLength > MAX_INLINE_CIPHER) {
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
