import { describe, expect, it } from "vitest";

import {
  audienceOf,
  chosenGroups,
  deletePatch,
  groupPatch,
  type GroupForm,
  movedIn,
  nameProblem,
  settingNames,
  settingsText,
  notSetBy,
  uploadsText,
  uploadDraft,
  uploadOf,
  type Upload,
  withAudience,
  withGroup,
  withoutGroup,
  withShares
} from "./groups";

describe("who sees each share", () => {
  const map = { Rare: ["buddies"], private: ["jazz", "family"] };

  it("is looked up and replaced by alias in any case", () => {
    expect(audienceOf(map, "RARE")).toEqual(["buddies"]);
    expect(audienceOf(map, "music")).toBeNull();
    expect(withAudience(map, "rare", ["jazz"])).toEqual({
      private: ["jazz", "family"],
      rare: ["jazz"]
    });
    expect(withAudience(map, "PRIVATE", null)).toEqual({ Rare: ["buddies"] });
  });

  it("follows a group's shares, a share everyone saw becoming the group's alone", () => {
    expect(withShares(map, "jazz", ["rare", "music"])).toEqual({
      Rare: ["buddies", "jazz"],
      private: ["family"],
      music: ["jazz"]
    });
  });
});

describe("the order of the groups", () => {
  it("moves one group up or down past the others", () => {
    const order = ["a", "b", "c", "d"];
    expect(movedIn(order, 3, 1)).toEqual(["a", "d", "b", "c"]);
    expect(movedIn(order, 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(movedIn(order, 1, 1)).toEqual(order);
  });
});

describe("group names", () => {
  it("are short words that no built-in or other group has", () => {
    expect(nameProblem("close-friends", ["jazz"])).toBeNull();
    expect(nameProblem("", [])).not.toBeNull();
    expect(nameProblem("Close friends", [])).not.toBeNull();
    expect(nameProblem("x".repeat(33), [])).not.toBeNull();
    expect(nameProblem("buddies", [])).toBe("buddies is a built-in group");
    expect(nameProblem("jazz", ["jazz"])).not.toBeNull();
  });
});

describe("a user's groups", () => {
  it("are the manual ones we chose, the buddies left to the buddy list", () => {
    const groups = [
      { name: "privileged", kind: "smart", defined: true, uploadSettings: false },
      { name: "buddies", kind: "manual", defined: true, uploadSettings: false },
      { name: "jazz", kind: "manual", defined: true, uploadSettings: true },
      { name: "gone", kind: "manual", defined: false, uploadSettings: false }
    ] as const;
    const chosen = chosenGroups([...groups]);
    expect(chosen).toEqual(["jazz", "gone"]);
    expect(withGroup(chosen, "jazz")).toEqual(["jazz", "gone"]);
    expect(withGroup(chosen, "bop")).toEqual(["jazz", "gone", "bop"]);
    expect(withoutGroup(chosen, "gone")).toEqual(["jazz"]);
  });
});

describe("a group's upload settings", () => {
  const upload = {
    strategy: "firstinfirstout",
    slots: 0,
    speed_limit: null,
    limits: {
      queued: { files: null, megabytes: 500, failures: null },
      daily: { files: 0, megabytes: null, failures: null },
      weekly: { files: null, megabytes: null, failures: 3 }
    }
  };

  it("come back from their draft whole, not set apart from no limit", () => {
    expect(uploadOf(uploadDraft(upload))).toEqual({ upload });
    const unset = uploadOf(uploadDraft(null));
    expect(unset).toEqual({
      upload: {
        strategy: null,
        slots: null,
        speed_limit: null,
        limits: Object.fromEntries(
          ["queued", "daily", "weekly"].map((w) => [
            w,
            { files: null, megabytes: null, failures: null }
          ])
        )
      }
    });
  });

  it("refuse a number a person can fix before saving", () => {
    expect(uploadOf({ ...uploadDraft(null), slots: "-1" })).toEqual({
      error: "Upload slots must be a whole number of at least 0"
    });
    expect(uploadOf({ ...uploadDraft(null), speed_limit: "2.5" })).toHaveProperty("error");
  });

  it("say what they set and nothing of what they leave to others", () => {
    expect(settingsText(upload)).toEqual([
      "First in, first out",
      "Every slot",
      "Queued 500 MB",
      "No daily file limit",
      "Weekly 3 failures"
    ]);
    expect(settingsText({})).toEqual([]);
    expect(notSetBy(settingNames(upload), { slots: 2, limits: { daily: { files: 5 } } })).toEqual([
      "queue order",
      "queued size limit",
      "weekly failure limit"
    ]);
  });
});

describe("what a user's uploads get", () => {
  it("names the group each setting comes from", () => {
    expect(
      uploadsText({
        queue: "buddies",
        slots: null,
        speed: "default",
        speedLimit: 200,
        limits: [{ window: "daily", measure: "files", limit: 10, group: "leechers" }]
      })
    ).toBe("Queue: buddies (every slot) · Speed: 200 KiB/s (Everyone) · Daily 10 files (leechers)");
  });
});

describe("the options a group's save writes", () => {
  const values = {
    transfers: {
      groups: {
        order: ["buddies", "jazz", "leechers"],
        user_defined: { jazz: { color: "teal", upload: null }, bop: { color: "red" } },
        buddies: { upload: { slots: 0, speed_limit: 0 } },
        leechers: { upload: { limits: { daily: { files: 10 } } } }
      }
    },
    shares: { visible_to: { Rare: ["jazz"], live: ["jazz", "buddies"] } }
  };
  const form = (fields: Partial<GroupForm>): GroupForm => ({
    from: "jazz",
    own: true,
    name: "jazz",
    color: "teal",
    draft: uploadDraft(null),
    started: uploadDraft(null),
    seen: ["Rare", "live"],
    ...fields
  });
  const written = (patch: ReturnType<typeof groupPatch>) => {
    if ("error" in patch) throw new Error(patch.error);
    return patch;
  };

  it("takes a renamed group's name into the order and the shares", () => {
    const patch = written(groupPatch(values, form({ name: "swing" })));
    expect(patch.set["transfers.groups.order"]).toEqual(["buddies", "swing", "leechers"]);
    expect(patch.set["shares.visible_to"]).toEqual({ Rare: ["swing"], live: ["buddies", "swing"] });
    expect(Object.keys(patch.set["transfers.groups.user_defined"] as object)).toEqual([
      "bop",
      "swing"
    ]);
    expect(patch.hidden).toEqual([]);
  });

  it("writes a user-defined group whole, null for what it leaves not set", () => {
    const draft = { ...uploadDraft(null), slots: "3" };
    const patch = written(groupPatch(values, form({ draft })));
    const jazz = (patch.set["transfers.groups.user_defined"] as Record<string, Upload>).jazz;
    expect(jazz.color).toBe("teal");
    expect((jazz.upload as Upload).slots).toBe(3);
    expect((jazz.upload as Upload).speed_limit).toBeNull();
    expect(patch.set["transfers.groups.order"]).toBeUndefined();
  });

  it("names the shares an unticked group leaves seen by no one", () => {
    const patch = written(groupPatch(values, form({ seen: ["live"] })));
    expect(patch.set["shares.visible_to"]).toEqual({ Rare: [], live: ["buddies", "jazz"] });
    expect(patch.hidden).toEqual(["Rare"]);
  });

  it("takes a built-in group's setting out when it is no longer set", () => {
    const started = uploadDraft({ limits: { daily: { files: 10 } } });
    const draft = { ...started, "limits.daily.files": "", "limits.weekly.files": "0" };
    const patch = written(
      groupPatch(
        values,
        form({ from: "leechers", name: "leechers", own: false, seen: null, draft, started })
      )
    );
    expect(patch.unset).toEqual(["transfers.groups.leechers.upload.limits.daily.files"]);
    expect(patch.set).toEqual({ "transfers.groups.leechers.upload.limits.weekly.files": 0 });
  });

  it("never takes out the buddies' slots or speed limit, whose empty is no limit", () => {
    const started = uploadDraft({ slots: 4, speed_limit: 0 });
    const draft = { ...started, slots: "", speed_limit: "" };
    const patch = written(
      groupPatch(
        values,
        form({ from: "buddies", name: "buddies", own: false, seen: [], draft, started })
      )
    );
    expect(patch.unset).toEqual([]);
    expect(patch.set["transfers.groups.buddies.upload.slots"]).toBe(0);
    expect(patch.set["transfers.groups.buddies.upload.speed_limit"]).toBeUndefined();
  });

  it("refuses a threshold a person can fix", () => {
    const thresholds = { files: "0", directories: "1", was: { files: "1", directories: "1" } };
    const patch = groupPatch(
      values,
      form({ from: "leechers", name: "leechers", own: false, seen: null, thresholds })
    );
    expect(patch).toEqual({ error: "Minimum shared files must be a whole number of at least 1" });
  });
});

describe("deleting a group", () => {
  it("takes it out of the groups and the order, and keeps its shares listed for no one", () => {
    const values = {
      transfers: { groups: { order: ["buddies", "jazz", "leechers"], user_defined: { jazz: {} } } },
      shares: { visible_to: { Rare: ["jazz"], live: ["jazz", "buddies"] } }
    };
    expect(deletePatch(values, "jazz")).toEqual({
      set: {
        "transfers.groups.user_defined": {},
        "transfers.groups.order": ["buddies", "leechers"],
        "shares.visible_to": { Rare: [], live: ["buddies"] }
      },
      unset: [],
      hidden: ["Rare"]
    });
  });
});
