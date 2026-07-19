import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  // Eine Sync-Gruppe = ein gemeinsames Secret. Der Server kennt nur
  // die (unratbare) groupId und den Hash des Auth-Keys — nie Schlüssel oder Klartext.
  groups: defineTable({
    groupId: v.string(),
    authKeyHash: v.string(), // hex(sha256(authKey))
    createdAt: v.number(),
  }).index("by_groupId", ["groupId"]),

  // Verschlüsselte Einträge (cipher/thumbCipher sind AES-256-GCM-Blobs).
  // `seq` ist der SERVERSEITIG vergebene, pro Gruppe monotone Pull-Cursor —
  // Client-Lamports taugen nicht als Cursor (Gleichstände, verspätete Pushes).
  entries: defineTable({
    groupId: v.string(),
    uuid: v.string(),
    kind: v.number(), // 0 Text, 1 Bild, 2 Dateien, 3 Settings
    cipher: v.optional(v.bytes()),
    thumbCipher: v.optional(v.bytes()),
    pinned: v.boolean(),
    createdAt: v.number(),
    deleted: v.boolean(),
    lamport: v.number(),
    deviceId: v.string(),
    seq: v.number(),
  })
    .index("by_group_seq", ["groupId", "seq"])
    .index("by_group_uuid", ["groupId", "uuid"]),
});
