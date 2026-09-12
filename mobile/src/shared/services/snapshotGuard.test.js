import { expect, test } from "bun:test";
import { SnapshotGuard } from "./snapshotGuard.ts";

function deferred() {
    let resolve;
    const promise = new Promise((done) => {
        resolve = done;
    });
    return { promise, resolve };
}

test("a pre-leave response cannot restore a session after the leave completes", async () => {
    const guard = new SnapshotGuard();
    const remote = deferred();
    const stale = guard.read(() => remote.promise);
    await guard.mutate(async () => "left group");
    remote.resolve({ session: "old family" });
    expect(await stale).toBeNull();
    expect(await guard.read(async () => ({ session: null }))).toEqual({ session: null });
});

test("reads wait for mutations to settle and recover after a failed mutation", async () => {
    const guard = new SnapshotGuard();
    const remote = deferred();
    const mutation = guard.mutate(() => remote.promise);
    let called = false;
    expect(
        await guard.read(async () => {
            called = true;
        }),
    ).toBeNull();
    expect(called).toBe(false);
    remote.resolve();
    await mutation;
    await expect(
        guard.mutate(async () => {
            throw new Error("Offline");
        }),
    ).rejects.toThrow("Offline");
    expect(await guard.read(async () => "saved state")).toBe("saved state");
});

test("an older polling result cannot overwrite a newer location response", async () => {
    const guard = new SnapshotGuard();
    const remote = deferred();
    const first = guard.read(() => remote.promise);
    expect(await guard.read(async () => "current location")).toBe("current location");
    remote.resolve("old location");
    expect(await first).toBeNull();
});

test("mutations preserve request order even when the first response is delayed", async () => {
    const guard = new SnapshotGuard();
    const remote = deferred();
    const calls = [];
    const first = guard.mutate(async () => {
        calls.push("first");
        return remote.promise;
    });
    const second = guard.mutate(async () => {
        calls.push("second");
        return "latest";
    });
    await Promise.resolve();
    expect(calls).toEqual(["first"]);
    expect(await guard.read(async () => "stale")).toBeNull();
    remote.resolve("earlier");
    expect(await first).toBe("earlier");
    expect(await second).toBe("latest");
    expect(calls).toEqual(["first", "second"]);
});

test("a rejected mutation does not block the next queued action", async () => {
    const guard = new SnapshotGuard();
    const first = guard.mutate(async () => {
        throw new Error("Unavailable");
    });
    const second = guard.mutate(async () => "recovered");
    await expect(first).rejects.toThrow("Unavailable");
    expect(await second).toBe("recovered");
    expect(await guard.read(async () => "current")).toBe("current");
});
