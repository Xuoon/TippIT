import { sha256 } from "js-sha256";
import type { MutationCtx, QueryCtx } from "./_generated/server";

/** Auth ohne Accounts: authKey wird nur als Hash gespeichert;
 * ein DB-Leak erlaubt damit keine Schreibzugriffe. */
export async function requireGroup(
  ctx: QueryCtx | MutationCtx,
  groupId: string,
  authKey: string
) {
  const group = await ctx.db
    .query("groups")
    .withIndex("by_groupId", (q) => q.eq("groupId", groupId))
    .unique();
  if (!group || sha256(authKey) !== group.authKeyHash) {
    throw new Error("Ungültige Gruppe oder Auth-Key");
  }
  return group;
}
