import {
    useCallback,
    useEffect,
    useMemo,
    useReducer,
    useRef,
    useState,
    type ReactNode,
} from "react";

import { LumoContext } from "@app/state/lumoContext.ts";
import { BackendError, lumoBackend } from "@shared/services/lumoBackend.ts";
import { parseCoordinates } from "@shared/utils/coordinates.ts";

import {
    browserStorage,
    createInitialState,
    reducer,
    removeStored,
    STORAGE_KEYS,
    writeStored,
} from "@app/state/lumoState.ts";

export function LumoProvider({ children }: { children: ReactNode }) {
    const [state, dispatch] = useReducer(reducer, undefined, () =>
        createInitialState(lumoBackend.isNative()),
    );
    const notifiedEventIds = useRef<Set<string> | null>(null);
    const requestedControllerPermissions = useRef(false);
    const stateRef = useRef(state);
    const trackerRecoveryInFlight = useRef(false);
    const [bootstrapStatus, setBootstrapStatus] = useState<"loading" | "ready" | "error">(
        lumoBackend.isNative() ? "loading" : "ready",
    );
    const [bootstrapError, setBootstrapError] = useState("");
    const [bootstrapCanReset, setBootstrapCanReset] = useState(false);
    const retryRef = useRef<() => void>(() => undefined);
    const bootstrapped = useRef(!lumoBackend.isNative());
    const retryBootstrap = useCallback(() => retryRef.current(), []);

    stateRef.current = state;

    useEffect(() => {
        if (lumoBackend.isNative()) return;
        const storage = browserStorage("localStorage");
        if (!storage) return;
        writeStored(storage, STORAGE_KEYS.schema, 8);
        writeStored(storage, STORAGE_KEYS.mode, state.mode);
        writeStored(storage, STORAGE_KEYS.demo, state.demo);
        writeStored(storage, STORAGE_KEYS.places, state.places);
        writeStored(storage, STORAGE_KEYS.events, state.events);
        writeStored(storage, STORAGE_KEYS.preferences, state.preferences);
    }, [state.mode, state.demo, state.places, state.events, state.preferences]);

    useEffect(() => {
        if (lumoBackend.isNative()) return;
        const storage = browserStorage("localStorage");
        if (!storage) return;
        if (!state.group.active) {
            removeStored(storage, STORAGE_KEYS.group);
            return;
        }
        writeStored(storage, STORAGE_KEYS.group, state.group);
    }, [state.group]);

    useEffect(() => {
        dispatch({ type: "PURGE_OLD_EVENTS" });
        const interval = window.setInterval(() => dispatch({ type: "PURGE_OLD_EVENTS" }), 60_000);
        return () => window.clearInterval(interval);
    }, []);

    useEffect(() => {
        if (!lumoBackend.isNative()) return;
        let active = true;
        let inFlight = false;
        let timeout: number | undefined;
        const synchronize = async () => {
            if (!active || inFlight) return;
            if (bootstrapped.current && document.visibilityState === "hidden") return;
            window.clearTimeout(timeout);
            inFlight = true;
            try {
                const snapshot = await lumoBackend.bootstrap(state.mode);
                if (active && snapshot) {
                    if (
                        snapshot.group.active &&
                        snapshot.group.role === "member" &&
                        lumoBackend.isMobileNative() &&
                        (!state.group.active || stateRef.current.mobile === null)
                    ) {
                        const mobile = await lumoBackend.getMobileStatus();
                        if (!active) return;
                        if (!mobile)
                            throw new Error(
                                "No se ha podido comprobar la configuración de Android. Vuelve a intentarlo",
                            );
                        dispatch({ type: "SYNC_MOBILE_STATUS", payload: mobile });
                    }
                    bootstrapped.current = true;
                    setBootstrapStatus("ready");
                    setBootstrapError("");
                    setBootstrapCanReset(false);
                    dispatch({
                        type: "HYDRATE_BACKEND",
                        payload:
                            state.group.active && state.mode === null
                                ? { ...snapshot, mode: null }
                                : snapshot,
                    });
                }
            } catch (error) {
                if (active && !bootstrapped.current) {
                    setBootstrapStatus("error");
                    setBootstrapCanReset(
                        error instanceof BackendError && error.code === "session_recovery_required",
                    );
                    setBootstrapError(
                        error instanceof Error
                            ? error.message
                            : "No se ha podido recuperar la configuración",
                    );
                }
            } finally {
                inFlight = false;
                if (active) timeout = window.setTimeout(synchronize, 5_000);
            }
        };
        const onVisible = () => {
            if (document.visibilityState === "visible") void synchronize();
        };
        retryRef.current = () => {
            if (!bootstrapped.current) setBootstrapStatus("loading");
            void synchronize();
        };
        void synchronize();
        document.addEventListener("visibilitychange", onVisible);
        return () => {
            active = false;
            window.clearTimeout(timeout);
            document.removeEventListener("visibilitychange", onVisible);
        };
    }, [state.group.active, state.mode]);

    useEffect(() => {
        if (!lumoBackend.isMobileNative()) return;
        let active = true;
        let inFlight = false;
        const synchronize = async () => {
            if (!active || inFlight) return;
            if (document.visibilityState === "hidden") return;
            inFlight = true;
            try {
                const status = await lumoBackend.getMobileStatus();
                if (!active || !status) return;
                dispatch({ type: "SYNC_MOBILE_STATUS", payload: status });

                const current = stateRef.current;
                const canRecoverTracker =
                    current.group.active &&
                    current.group.role === "member" &&
                    status.controlledTrackingConfigured &&
                    !status.trackingEnabled &&
                    status.controlledTrackingMayAutoRecover &&
                    status.preciseLocation === "granted" &&
                    status.backgroundLocation !== "denied" &&
                    status.notifications === "granted" &&
                    status.locationServicesEnabled;
                if (!canRecoverTracker || trackerRecoveryInFlight.current) return;

                trackerRecoveryInFlight.current = true;
                try {
                    const result = await lumoBackend.setControlledTracking(true);
                    if (active && result.status) {
                        dispatch({ type: "SYNC_MOBILE_STATUS", payload: result.status });
                    }
                    if (active && result.snapshot) {
                        dispatch({ type: "HYDRATE_BACKEND", payload: result.snapshot });
                    }
                } finally {
                    trackerRecoveryInFlight.current = false;
                }
            } catch {
                // Android settings can be temporarily unavailable while another activity is open.
            } finally {
                inFlight = false;
            }
        };
        const onVisible = () => {
            if (document.visibilityState === "visible") void synchronize();
        };
        void synchronize();
        const interval = window.setInterval(synchronize, 15_000);
        document.addEventListener("visibilitychange", onVisible);
        window.addEventListener("focus", synchronize);
        return () => {
            active = false;
            window.clearInterval(interval);
            document.removeEventListener("visibilitychange", onVisible);
            window.removeEventListener("focus", synchronize);
        };
    }, []);

    useEffect(() => {
        if (state.mode !== "controller") {
            requestedControllerPermissions.current = false;
        }
        if (
            !state.group.active ||
            state.mode !== "controller" ||
            !lumoBackend.isMobileNative() ||
            state.mobile === null ||
            requestedControllerPermissions.current
        ) {
            return;
        }
        requestedControllerPermissions.current = true;
        if (!state.preferences.notifications) return;
        void lumoBackend
            .requestMobilePermissions("controller")
            .then(async () => {
                const current = stateRef.current;
                if (
                    !current.group.active ||
                    current.mode !== "controller" ||
                    !current.preferences.notifications
                )
                    return;
                const status = await lumoBackend.configureMobileTracking("controller", true);
                if (
                    status &&
                    stateRef.current.group.active &&
                    stateRef.current.mode === "controller"
                ) {
                    dispatch({ type: "SYNC_MOBILE_STATUS", payload: status });
                }
            })
            .catch(() => undefined);
    }, [state.group.active, state.mobile, state.mode, state.preferences.notifications]);

    useEffect(() => {
        if (
            state.mode !== "controller" ||
            !state.preferences.notifications ||
            !lumoBackend.isMobileNative()
        ) {
            notifiedEventIds.current = new Set(state.events.map((event) => event.id));
            return;
        }
        const known = notifiedEventIds.current;
        if (!known) {
            notifiedEventIds.current = new Set(state.events.map((event) => event.id));
            return;
        }
        const now = Date.now();
        const fresh = state.events.filter(
            (event) =>
                !event.read &&
                !known.has(event.id) &&
                now - new Date(event.at).getTime() <= 2 * 60_000,
        );
        notifiedEventIds.current = new Set(state.events.map((event) => event.id));
        fresh.forEach((event) => {
            if (event.kind === "help") {
                void lumoBackend
                    .startEmergencyAlarm(event.id, event.title, event.detail, {
                        phone: state.group.trackedPersonPhone,
                        address: state.demo.address || undefined,
                        ...(() => {
                            const coordinates = state.demo.coordinates
                                ? parseCoordinates(state.demo.coordinates)
                                : null;
                            return coordinates
                                ? {
                                      latitude: coordinates.latitude,
                                      longitude: coordinates.longitude,
                                  }
                                : {};
                        })(),
                    })
                    .catch(() => undefined);
            } else {
                void lumoBackend
                    .showNotification(event.title, event.detail, {
                        id: event.id,
                        urgent: false,
                    })
                    .catch(() => undefined);
            }
        });
    }, [
        state.demo.address,
        state.demo.coordinates,
        state.events,
        state.group.trackedPersonPhone,
        state.mode,
        state.preferences.notifications,
    ]);

    const value = useMemo(
        () => ({
            state,
            dispatch,
            backend: lumoBackend,
            bootstrapStatus,
            bootstrapError,
            bootstrapCanReset,
            retryBootstrap,
        }),
        [state, bootstrapStatus, bootstrapError, bootstrapCanReset, retryBootstrap],
    );

    return <LumoContext.Provider value={value}>{children}</LumoContext.Provider>;
}
