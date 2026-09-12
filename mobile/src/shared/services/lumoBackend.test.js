import { afterEach, beforeEach, expect, test } from "bun:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { lumoBackend } from "./lumoBackend.ts";

const originalWindow = globalThis.window;
beforeEach(() => {
    globalThis.window = { navigator: { userAgent: "Desktop test" } };
});
afterEach(() => {
    clearMocks();
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
});

function snapshot(overrides = {}) {
    return {
        profile: "controller",
        session: null,
        places: [],
        events: [],
        controlled: {
            lastSeenAtMs: Date.now(),
            connectivity: "online",
            precisePermission: "granted",
            trackingEnabled: true,
            batteryPercent: 80,
            lastLocation: null,
            ...overrides,
        },
    };
}

test("a false native PIN result rejects protected actions", async () => {
    mockIPC(() => ({ verified: false }));
    await expect(lumoBackend.verifyPin("123456")).rejects.toThrow("El PIN no es correcto");
});

test("malformed PINs never reach native IPC", async () => {
    let calls = 0;
    mockIPC(() => {
        calls += 1;
        return { verified: true };
    });
    await expect(lumoBackend.verifyPin("123")).rejects.toThrow("6 cifras");
    expect(calls).toBe(0);
});

test("editing a place preserves its configured geofence radius", async () => {
    let input;
    mockIPC((command, args) => {
        expect(command).toBe("place_update");
        input = args.input;
        return { id: args.id, ...args.input };
    });
    const result = await lumoBackend.savePlace(
        {
            id: "home",
            name: "Casa",
            address: "Main street",
            coordinates: "40.4, -3.7",
            radius: 250,
            kind: "home",
            color: "purple",
            icon: "home",
        },
        true,
    );
    expect(input.radiusM).toBe(250);
    expect(result.radius).toBe(250);
});

test("a heartbeat with an old position does not claim a new location capture", async () => {
    const capturedAtMs = Date.now() - 600_000;
    mockIPC(() =>
        snapshot({
            lastLocation: {
                latitude: 40,
                longitude: -3,
                accuracyM: 10,
                capturedAtMs,
                batteryPercent: 80,
            },
        }),
    );
    const result = await lumoBackend.bootstrap("controller");
    expect(Date.parse(result.demo.lastUpdatedAt)).toBe(capturedAtMs);
});

test("a device without a first heartbeat is offline", async () => {
    mockIPC(() => snapshot({ lastSeenAtMs: null }));
    expect((await lumoBackend.bootstrap("controller")).demo.connection).toBe("offline");
});

test("replacement invitations target exactly the selected controlled device", async () => {
    const calls = [];
    mockIPC((command, args) => {
        calls.push([command, args]);
        return { invitationId: "invite" };
    });
    await lumoBackend.createInvitation("123456", "controlled", "old-device");
    expect(calls).toEqual([
        [
            "group_create_invitation",
            { pin: "123456", role: "controlled", replaceDeviceId: "old-device" },
        ],
    ]);
});

test("an unverified invitation never proceeds to session bootstrap", async () => {
    const calls = [];
    mockIPC((command) => {
        calls.push(command);
        return { verified: false, role: "controlled" };
    });
    await expect(lumoBackend.joinGroup("invite", "token", "123456")).rejects.toThrow(
        "verificar la invitación",
    );
    expect(calls).toEqual(["group_consume_invitation"]);
});

test("leaving waits for pending tracking permissions and stops after configuration finishes", async () => {
    globalThis.window.navigator.userAgent = "Android";
    let allowPermissions;
    const permission = new Promise((resolve) => {
        allowPermissions = resolve;
    });
    const status = {
        role: "controlled",
        preciseLocation: "granted",
        backgroundLocation: "granted",
        locationServicesEnabled: true,
    };
    const calls = [];
    mockIPC(async (command) => {
        calls.push(command);
        if (command === "mobile_request_permissions") return permission;
        if (command === "tracker_set_tracking") return snapshot();
        if (command === "mobile_configure_tracking") return status;
        return null;
    });
    const enabling = lumoBackend.setControlledTracking(true);
    const leaving = lumoBackend.leaveGroup("123456");
    await Promise.resolve();
    expect(calls).toEqual(["mobile_request_permissions"]);
    allowPermissions(status);
    await enabling;
    await leaving;
    expect(calls).toEqual([
        "mobile_request_permissions",
        "tracker_set_tracking",
        "mobile_configure_tracking",
        "group_leave",
    ]);
});

test("session restoration errors preserve the dedicated recovery code", async () => {
    mockIPC(() => {
        throw { code: "session_recovery_required", message: "vault inaccessible" };
    });
    try {
        await lumoBackend.bootstrap(null);
        throw new Error("Expected restoration failure");
    } catch (error) {
        expect(error.code).toBe("session_recovery_required");
        expect(error.message).toContain("antes de vincular de nuevo");
    }
});

test("resetting a failed local session uses only the explicit reset command", async () => {
    const calls = [];
    mockIPC((command) => {
        calls.push(command);
        return null;
    });
    await lumoBackend.resetLocalSession();
    expect(calls).toEqual(["app_reset_local_session"]);
});
