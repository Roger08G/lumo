import { afterEach, describe, expect, test } from "bun:test";
import { createInitialState, reducer, STORAGE_KEYS } from "./lumoState.ts";

const originalWindow = globalThis.window;
afterEach(() => {
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
});

function storage(values = {}) {
    const items = new Map(
        Object.entries(values).map(([key, value]) => [key, JSON.stringify(value)]),
    );
    return {
        getItem: (key) => items.get(key) ?? null,
        removeItem: (key) => items.delete(key),
        setItem: (key, value) => items.set(key, value),
    };
}

function linkedState() {
    const state = createInitialState();
    return {
        ...state,
        group: {
            ...state.group,
            active: true,
            code: "GROUP1",
            name: "Family",
            role: "member",
            entry: "joined",
        },
        mode: "tracker",
        preferences: { ...state.preferences, trackerSetupComplete: true },
    };
}

function hydration(state, overrides = {}) {
    return {
        group: state.group,
        mode: state.mode,
        places: state.places,
        events: state.events,
        demo: state.demo,
        trackerSetupComplete: false,
        ...overrides,
    };
}

describe("safe startup and family state", () => {
    test("restricted WebView storage cannot prevent startup", () => {
        globalThis.window = {
            get localStorage() {
                throw new Error("Access denied");
            },
            get sessionStorage() {
                throw new Error("Access denied");
            },
        };
        expect(createInitialState(true).group.active).toBe(false);
        expect(createInitialState(false).places.length).toBeGreaterThan(0);
    });

    test("valid JSON with corrupt shapes falls back without a render crash", () => {
        globalThis.window = {
            localStorage: storage({
                [STORAGE_KEYS.schema]: 8,
                [STORAGE_KEYS.group]: null,
                [STORAGE_KEYS.places]: {},
                [STORAGE_KEYS.events]: [null],
                [STORAGE_KEYS.preferences]: { trackerConsents: null },
            }),
            sessionStorage: storage(),
        };
        const state = createInitialState();
        expect(state.group.active).toBe(false);
        expect(Array.isArray(state.places)).toBe(true);
        expect(state.preferences.trackerConsents.preciseLocation).toBe(false);
    });

    test("native startup removes private browser snapshots and never hydrates them", () => {
        const previous = linkedState();
        const localStorage = storage({
            [STORAGE_KEYS.schema]: 8,
            [STORAGE_KEYS.group]: previous.group,
            [STORAGE_KEYS.events]: previous.events,
        });
        globalThis.window = { localStorage, sessionStorage: storage() };
        expect(createInitialState(true).group.active).toBe(false);
        expect(localStorage.getItem(STORAGE_KEYS.group)).toBeNull();
        expect(localStorage.getItem(STORAGE_KEYS.events)).toBeNull();
    });

    test("pausing tracking preserves completed setup for the same family", () => {
        const state = linkedState();
        const next = reducer(state, { type: "HYDRATE_BACKEND", payload: hydration(state) });
        expect(next.preferences.trackerSetupComplete).toBe(true);
        expect(next.mode).toBe("tracker");
    });

    test("a restarted, deliberately paused phone keeps setup without implying automatic recovery", () => {
        const state = createInitialState();
        const next = reducer(state, {
            type: "SYNC_MOBILE_STATUS",
            payload: {
                role: "controlled",
                trackingEnabled: false,
                controlledTrackingConfigured: true,
                controlledTrackingMayAutoRecover: false,
                preciseLocation: "granted",
                backgroundLocation: "granted",
            },
        });
        expect(next.preferences.trackerSetupComplete).toBe(true);
        expect(next.mobile.trackingEnabled).toBe(false);
        expect(next.mobile.controlledTrackingMayAutoRecover).toBe(false);
    });

    test("authoritative group and role replace stale local identity", () => {
        const state = linkedState();
        const group = {
            ...state.group,
            code: "GROUP2",
            role: "supervisor",
            userName: "New supervisor",
        };
        const next = reducer(state, {
            type: "HYDRATE_BACKEND",
            payload: hydration(state, { group, mode: "controller" }),
        });
        expect(next.group).toEqual(group);
        expect(next.preferences.trackerSetupComplete).toBe(false);
    });

    test("leaving removes family events, places, mobile state and permissions", () => {
        const next = reducer(linkedState(), { type: "LEAVE_GROUP" });
        expect(next.group.active).toBe(false);
        expect(next.events).toEqual([]);
        expect(next.places).toEqual([]);
        expect(next.mobile).toBeNull();
        expect(next.preferences.trackerSetupComplete).toBe(false);
    });

    test("a new family cannot inherit a resolved private address at identical coordinates", () => {
        const state = linkedState();
        state.demo = { ...state.demo, coordinates: "40, -3", address: "Previous private address" };
        const payload = hydration(state, {
            group: { ...state.group, code: "GROUP2" },
            demo: { ...state.demo, address: "" },
        });
        expect(reducer(state, { type: "HYDRATE_BACKEND", payload }).demo.address).toBe("");
    });
});
