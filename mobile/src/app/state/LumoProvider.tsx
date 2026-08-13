import { useEffect, useMemo, useReducer, type ReactNode } from "react";

import { LumoContext } from "@app/state/lumoContext.ts";
import { lumoBackend } from "@shared/services/lumoBackend.ts";

import type {
    DebugScenario,
    DemoState,
    GroupState,
    LumoAction,
    LumoState,
    Place,
    PreferencesState,
    TimelineEvent,
} from "@shared/types/lumo.ts";

const STORAGE_KEYS = {
    schema: "lumo.schema-version",
    group: "lumo.group",
    mode: "lumo.mode",
    demo: "lumo.demo-state",
    places: "lumo.places",
    events: "lumo.events",
    preferences: "lumo.preferences",
} as const;

const EMPTY_GROUP: GroupState = {
    active: false,
    name: "",
    code: "",
    userName: "",
    supervisorName: "",
    trackedPersonName: "",
    role: null,
    entry: null,
};

const DEFAULT_PLACES: Place[] = [
    {
        id: "home",
        name: "Casa",
        address: "Dirección principal",
        coordinates: "40.4168, -3.7038",
        radius: 120,
        kind: "home",
        color: "purple",
        icon: "home",
    },
    {
        id: "supermarket",
        name: "Supermercado",
        address: "Dirección habitual",
        coordinates: "40.4191, -3.7072",
        radius: 90,
        kind: "shop",
        color: "yellow",
        icon: "shopping",
    },
    {
        id: "medical",
        name: "Centro médico",
        address: "Dirección sanitaria",
        coordinates: "40.4154, -3.7061",
        radius: 100,
        kind: "medical",
        color: "pink",
        icon: "health",
    },
];

const DEFAULT_PREFERENCES: PreferencesState = {
    notifications: true,
    trackerSetupComplete: false,
    trackerConsents: {
        preciseLocation: false,
        backgroundLocation: false,
        batteryProtection: false,
    },
};

const createDefaultDemo = (): DemoState => ({
    location: "home",
    placeName: "Casa",
    statusText: "Está en casa",
    sinceLabel: "Desde hace 1 h 24 min",
    lastUpdatedAt: new Date().toISOString(),
    battery: 68,
    connection: "online",
    permission: "granted",
    accuracy: "high",
    delaySeconds: 2,
    lastTrip: {
        from: "Supermercado",
        to: "Casa",
        minutes: 18,
    },
});

const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
const EVENT_TTL_MS = 24 * 60 * 60 * 1000;

const recentEvents = (events: TimelineEvent[]) =>
    events.filter((event) => Date.now() - new Date(event.at).getTime() < EVENT_TTL_MS);

const createDefaultEvents = (): TimelineEvent[] => [
    {
        id: "initial-home",
        kind: "arrival",
        title: "Ha llegado a casa",
        detail: "El trayecto ha durado 18 minutos",
        at: minutesAgo(84),
        read: false,
    },
    {
        id: "initial-shop",
        kind: "departure",
        title: "Ha salido del supermercado",
        detail: "Destino probable: casa",
        at: minutesAgo(102),
        read: true,
    },
    {
        id: "initial-check",
        kind: "system",
        title: "Protección familiar activa",
        detail: "Ubicación y conexión disponibles",
        at: minutesAgo(240),
        read: true,
    },
];

function readStored<T>(storage: Storage, key: string, fallback: T): T {
    try {
        const value = storage.getItem(key);
        return value ? (JSON.parse(value) as T) : fallback;
    } catch {
        return fallback;
    }
}

function writeStored(storage: Storage, key: string, value: unknown) {
    try {
        storage.setItem(key, JSON.stringify(value));
    } catch {
        // The demo remains usable when private browsing or storage quotas block persistence.
    }
}

function createInitialState(): LumoState {
    const fallback: LumoState = {
        group: EMPTY_GROUP,
        mode: null,
        demo: createDefaultDemo(),
        places: DEFAULT_PLACES,
        events: createDefaultEvents(),
        preferences: DEFAULT_PREFERENCES,
    };

    if (typeof window === "undefined") return fallback;

    localStorage.removeItem("lumo.session");
    sessionStorage.removeItem("lumo.session");

    const storedSchema = readStored<number>(localStorage, STORAGE_KEYS.schema, 0);
    if (storedSchema < 8) {
        Object.values(STORAGE_KEYS).forEach((key) => localStorage.removeItem(key));
        localStorage.removeItem("lumo.preview-invite");
        return fallback;
    }

    const storedGroup = readStored<GroupState & { pin?: string }>(
        localStorage,
        STORAGE_KEYS.group,
        EMPTY_GROUP,
    );
    const { pin: _legacyPin, ...storedGroupWithoutPin } = storedGroup;
    const group =
        storedGroup.active && storedGroup.name && storedGroup.code
            ? {
                  ...storedGroupWithoutPin,
                  userName:
                      storedGroup.userName ||
                      (storedGroup.entry === "joined" ? "Miembro" : "Supervisor"),
                  supervisorName:
                      storedGroup.supervisorName ||
                      (storedGroup.entry === "joined"
                          ? "Supervisor"
                          : storedGroup.userName || "Supervisor"),
                  trackedPersonName: storedGroup.trackedPersonName || "Persona acompañada",
                  role:
                      storedGroup.role ||
                      (storedGroup.entry === "joined" ? "member" : "supervisor"),
              }
            : EMPTY_GROUP;
    const storedMode = readStored<LumoState["mode"]>(localStorage, STORAGE_KEYS.mode, null);
    const storedPlaces = readStored<Place[]>(localStorage, STORAGE_KEYS.places, fallback.places);
    const placeColors: Place["color"][] = ["purple", "yellow", "green", "blue", "pink"];
    const placeIcons: Place["icon"][] = [
        "home",
        "shopping",
        "health",
        "pin",
        "coffee",
        "school",
        "work",
        "park",
        "favorite",
        "activity",
    ];

    return {
        group,
        mode: group.active && group.role === "member" ? "tracker" : storedMode,
        demo: readStored(localStorage, STORAGE_KEYS.demo, fallback.demo),
        places: storedPlaces.map((place, index) => ({
            ...place,
            color: placeColors.includes(place.color)
                ? place.color
                : placeColors[index % placeColors.length],
            icon: placeIcons.includes(place.icon)
                ? place.icon
                : place.kind === "home"
                  ? "home"
                  : place.kind === "shop"
                    ? "shopping"
                    : place.kind === "medical"
                      ? "health"
                      : "pin",
        })),
        events: recentEvents(readStored(localStorage, STORAGE_KEYS.events, fallback.events)),
        preferences: readStored(localStorage, STORAGE_KEYS.preferences, fallback.preferences),
    };
}

function createEvent(kind: TimelineEvent["kind"], title: string, detail: string): TimelineEvent {
    return {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        kind,
        title,
        detail,
        at: new Date().toISOString(),
        read: false,
    };
}

function withEvent(state: LumoState, event: TimelineEvent, demo: DemoState): LumoState {
    return {
        ...state,
        demo,
        events: recentEvents([event, ...state.events]).slice(0, 40),
    };
}

function applyScenario(state: LumoState, scenario: DebugScenario): LumoState {
    const now = new Date().toISOString();
    const base = { ...state.demo, lastUpdatedAt: now };

    switch (scenario) {
        case "home":
            return withEvent(
                state,
                createEvent("arrival", "Ha llegado a casa", "Trayecto completado en 18 minutos"),
                {
                    ...base,
                    location: "home",
                    placeName: "Casa",
                    statusText: "Está en casa",
                    sinceLabel: "Desde hace unos instantes",
                    connection: "online",
                    permission: "granted",
                    lastTrip: { from: "Supermercado", to: "Casa", minutes: 18 },
                },
            );
        case "supermarket":
            return withEvent(
                state,
                createEvent(
                    "arrival",
                    "Ha llegado al supermercado",
                    "El trayecto ha durado 14 minutos",
                ),
                {
                    ...base,
                    location: "supermarket",
                    placeName: "Supermercado",
                    statusText: "Está en el supermercado",
                    sinceLabel: "Ha llegado ahora",
                    connection: "online",
                    permission: "granted",
                    lastTrip: { from: "Casa", to: "Supermercado", minutes: 14 },
                },
            );
        case "medical":
            return withEvent(
                state,
                createEvent(
                    "arrival",
                    "Ha llegado al centro médico",
                    "Trayecto completado sin avisos",
                ),
                {
                    ...base,
                    location: "medical",
                    placeName: "Centro médico",
                    statusText: "Está en el centro médico",
                    sinceLabel: "Ha llegado ahora",
                    connection: "online",
                    permission: "granted",
                    lastTrip: { from: "Casa", to: "Centro médico", minutes: 11 },
                },
            );
        case "away":
            return withEvent(
                state,
                createEvent("departure", "Ha salido de casa", "Lleva fuera menos de un minuto"),
                {
                    ...base,
                    location: "away",
                    placeName: "En trayecto",
                    statusText: "Está fuera de casa",
                    sinceLabel: "Ha salido ahora",
                    connection: "online",
                    permission: "granted",
                },
            );
        case "offline":
            return withEvent(
                state,
                createEvent(
                    "warning",
                    "No se recibe ubicación",
                    `Última ubicación conocida: ${state.demo.placeName}`,
                ),
                {
                    ...base,
                    connection: "offline",
                    statusText: "Conexión interrumpida",
                    sinceLabel: "Última señal hace 30 min",
                },
            );
        case "permission":
            return withEvent(
                state,
                createEvent(
                    "warning",
                    "Permiso de ubicación desactivado",
                    "La posición actual no está disponible",
                ),
                {
                    ...base,
                    permission: "revoked",
                    statusText: "Ubicación no disponible",
                    sinceLabel: "Requiere atención en el otro teléfono",
                },
            );
        case "battery":
            return withEvent(
                state,
                createEvent(
                    "warning",
                    "Batería baja",
                    `El teléfono de ${state.group.trackedPersonName || "la persona acompañada"} tiene un 12 %`,
                ),
                {
                    ...base,
                    battery: 12,
                },
            );
    }
}

function reducer(state: LumoState, action: LumoAction): LumoState {
    switch (action.type) {
        case "ENTER_GROUP": {
            const { pin: _pin, inviteToken: _inviteToken, ...group } = action.payload;
            return {
                ...state,
                group: { active: true, ...group },
                mode: action.payload.role === "supervisor" ? "controller" : "tracker",
                preferences:
                    action.payload.role === "member"
                        ? {
                              ...state.preferences,
                              trackerSetupComplete: false,
                              trackerConsents: {
                                  preciseLocation: false,
                                  backgroundLocation: false,
                                  batteryProtection: false,
                              },
                          }
                        : state.preferences,
            };
        }
        case "LEAVE_GROUP":
            return { ...state, group: EMPTY_GROUP, mode: null };
        case "HYDRATE_BACKEND":
            return {
                ...state,
                group:
                    state.group.active && action.payload.group.active
                        ? {
                              ...action.payload.group,
                              userName: state.group.userName,
                              role: state.group.role,
                              entry: state.group.entry,
                          }
                        : action.payload.group,
                mode: action.payload.mode,
                demo: action.payload.demo,
                places: action.payload.places,
                events: recentEvents(action.payload.events),
                preferences: {
                    ...state.preferences,
                    trackerSetupComplete: action.payload.trackerSetupComplete,
                },
            };
        case "SET_MODE":
            return { ...state, mode: action.payload };
        case "SET_TRACKER_CONSENT":
            return {
                ...state,
                preferences: {
                    ...state.preferences,
                    trackerConsents: {
                        ...state.preferences.trackerConsents,
                        [action.payload.key]: action.payload.value,
                    },
                },
            };
        case "COMPLETE_TRACKER_SETUP":
            return {
                ...state,
                preferences: { ...state.preferences, trackerSetupComplete: true },
            };
        case "SET_NOTIFICATIONS":
            return {
                ...state,
                preferences: { ...state.preferences, notifications: action.payload },
            };
        case "FINISH_LOCATE": {
            const demo = { ...state.demo, lastUpdatedAt: new Date().toISOString() };
            return withEvent(
                state,
                createEvent(
                    "location",
                    "Ubicación actualizada",
                    `${demo.placeName} · precisión aproximada de 12 m`,
                ),
                demo,
            );
        }
        case "APPLY_SCENARIO":
            return applyScenario(state, action.payload);
        case "SET_BATTERY":
            return { ...state, demo: { ...state.demo, battery: action.payload } };
        case "SET_CONNECTION":
            return { ...state, demo: { ...state.demo, connection: action.payload } };
        case "SET_PERMISSION":
            return { ...state, demo: { ...state.demo, permission: action.payload } };
        case "SET_ACCURACY":
            return { ...state, demo: { ...state.demo, accuracy: action.payload } };
        case "SET_DELAY":
            return { ...state, demo: { ...state.demo, delaySeconds: action.payload } };
        case "ADD_PLACE":
            return { ...state, places: [...state.places, action.payload] };
        case "UPDATE_PLACE":
            return {
                ...state,
                places: state.places.map((place) =>
                    place.id === action.payload.id ? action.payload : place,
                ),
            };
        case "DELETE_PLACE":
            return {
                ...state,
                places: state.places.filter((place) => place.id !== action.payload.id),
            };
        case "MARK_EVENTS_READ":
            return {
                ...state,
                events: state.events.map((event) => ({ ...event, read: true })),
            };
        case "PURGE_OLD_EVENTS": {
            const events = recentEvents(state.events);
            return events.length === state.events.length ? state : { ...state, events };
        }
        case "RESET_DEMO":
            return {
                ...state,
                demo: createDefaultDemo(),
                places: DEFAULT_PLACES,
                events: createDefaultEvents(),
            };
    }
}

export function LumoProvider({ children }: { children: ReactNode }) {
    const [state, dispatch] = useReducer(reducer, undefined, createInitialState);

    useEffect(() => {
        writeStored(localStorage, STORAGE_KEYS.schema, 8);
        writeStored(localStorage, STORAGE_KEYS.mode, state.mode);
        writeStored(localStorage, STORAGE_KEYS.demo, state.demo);
        writeStored(localStorage, STORAGE_KEYS.places, state.places);
        writeStored(localStorage, STORAGE_KEYS.events, state.events);
        writeStored(localStorage, STORAGE_KEYS.preferences, state.preferences);
    }, [state.mode, state.demo, state.places, state.events, state.preferences]);

    useEffect(() => {
        if (!state.group.active) {
            localStorage.removeItem(STORAGE_KEYS.group);
            return;
        }
        writeStored(localStorage, STORAGE_KEYS.group, state.group);
    }, [state.group]);

    useEffect(() => {
        dispatch({ type: "PURGE_OLD_EVENTS" });
        const interval = window.setInterval(() => dispatch({ type: "PURGE_OLD_EVENTS" }), 60_000);
        return () => window.clearInterval(interval);
    }, []);

    useEffect(() => {
        if (!lumoBackend.isNative()) return;
        let active = true;
        const synchronize = async () => {
            try {
                const snapshot = await lumoBackend.bootstrap(state.mode);
                if (active && snapshot) {
                    dispatch({
                        type: "HYDRATE_BACKEND",
                        payload:
                            state.group.active && state.mode === null
                                ? { ...snapshot, mode: null }
                                : snapshot,
                    });
                }
            } catch {
                // Preserve the last confirmed view during a temporary network interruption.
            }
        };
        void synchronize();
        const interval = window.setInterval(synchronize, 5_000);
        return () => {
            active = false;
            window.clearInterval(interval);
        };
    }, [state.group.active, state.mode]);

    const value = useMemo(() => ({ state, dispatch, backend: lumoBackend }), [state]);

    return <LumoContext.Provider value={value}>{children}</LumoContext.Provider>;
}
