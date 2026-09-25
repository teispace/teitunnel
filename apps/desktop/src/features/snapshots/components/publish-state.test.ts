import { describe, expect, it } from "vitest";
import type { PreparedView, SnapshotView, ZoneRef } from "@/lib/ipc/bindings";
import {
  changeFor,
  collectVars,
  initialDetails,
  initialSource,
  permissionNeeds,
  snapshotOptions,
  sourceReady,
} from "./publish-state";

const zones: ZoneRef[] = [{ id: "z1", name: "xyz.com" }];

const snapshot = {
  id: "s1",
  accountId: "acc",
  name: "docs",
  source: { type: "crawl", url: "http://localhost:3000" },
  access: { emails: ["me@xyz.com"], emailDomains: ["team.com"] },
  password: false,
  spa: true,
  comments: true,
} as unknown as SnapshotView;

const prepared = { id: "p1", suggestedName: "site", singlePage: false } as PreparedView;

describe("publishing a Snapshot", () => {
  it("starts from the Snapshot's own source and settings when updating", () => {
    const update = { kind: "update", snapshot } as const;
    expect(initialSource(update)).toMatchObject({ kind: "site", url: "http://localhost:3000" });
    expect(initialDetails(update, zones)).toMatchObject({
      protection: "login",
      allowed: "me@xyz.com, @team.com",
      spa: true,
      comments: true,
    });
    const fresh = { kind: "new", accountId: "acc" } as const;
    expect(initialSource(fresh).kind).toBe("folder");
    expect(initialDetails(fresh, zones)).toMatchObject({
      address: "domain",
      hostname: "preview.xyz.com",
    });
    expect(initialDetails(fresh, []).address).toBe("workersDev");
  });

  it("collects only a chosen source", () => {
    const base = { folder: null, project: null, url: "" };
    expect(collectVars({ ...base, kind: "keep" })).toBeNull();
    expect(collectVars({ ...base, kind: "folder" })).toBeNull();
    expect(sourceReady({ ...base, kind: "folder" })).toBe(false);
    expect(collectVars({ ...base, kind: "folder", folder: "/site" })).toEqual({
      kind: "folder",
      path: "/site",
    });
    expect(sourceReady({ ...base, kind: "site", url: "  " })).toBe(false);
    expect(sourceReady({ ...base, kind: "keep" })).toBe(true);
  });

  it("keeps, sets or removes the password", () => {
    const details = initialDetails({ kind: "new", accountId: "acc" }, zones);
    expect(snapshotOptions(details).password).toEqual({ type: "remove" });
    expect(snapshotOptions({ ...details, protection: "password" }).password).toEqual({
      type: "keep",
    });
    expect(
      snapshotOptions({ ...details, protection: "password", password: "s3cret" }).password,
    ).toEqual({ type: "set", password: "s3cret" });
    expect(snapshotOptions({ ...details, expires: "7" }).expiresInDays).toBe(7);
  });

  it("reviews a new Snapshot only once its files are collected", () => {
    const mode = { kind: "new", accountId: "acc" } as const;
    const details = { ...initialDetails(mode, zones), name: "docs" };
    expect(changeFor(mode, details, null, zones)).toBeNull();
    expect(changeFor(mode, details, prepared, zones)).toMatchObject({
      type: "publish",
      prepared: "p1",
      address: { type: "domain", hostname: "preview.xyz.com" },
    });
    expect(changeFor(mode, details, prepared, [])).toMatchObject({
      address: { type: "workersDev" },
    });
    // A new version may keep the files it has.
    expect(changeFor({ kind: "update", snapshot }, details, null, zones)).toMatchObject({
      type: "update",
      snapshot: "s1",
      prepared: null,
    });
  });

  it("asks for Workers Routes on the zone and Access for a login", () => {
    const details = initialDetails({ kind: "new", accountId: "acc" }, zones);
    expect(permissionNeeds(details, zones, false)).toEqual([
      { kind: "workers" },
      { kind: "workersRoutes", zone: "xyz.com" },
    ]);
    expect(permissionNeeds({ ...details, protection: "login" }, zones, true)).toEqual([
      { kind: "workers" },
      { kind: "access" },
    ]);
  });
});
