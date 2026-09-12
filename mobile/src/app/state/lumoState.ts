import type {
    DebugScenario,
    DemoState,
    GroupState,
    LumoAction,
    LumoState,
    MobileRuntimeStatus,
    Place,
    PreferencesState,
    TimelineEvent,
} from "@shared/types/lumo.ts";

export const STORAGE_KEYS = {
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
    supervisorPhone: "",
    trackedPersonName: "",
    trackedPersonPhone: "",
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
    coordinates: null,
    address: "",
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
    events.filter((event) => {
        const age = Date.now() - new Date(event.at).getTime();
        return age >= -60_000 && age < EVENT_TTL_MS;
    });

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
        const parsed: unknown = value ? JSON.parse(value) : null;
        return matchesStoredShape(parsed, fallback) ? (parsed as T) : fallback;
    } catch {
        return fallback;
    }
}

// Browser preview storage is untrusted: valid JSON can still have an invalid shape.
function matchesStoredShape(value: unknown, fallback: unknown): boolean {
    if (fallback === null) return value === null || typeof value === "string";
    if (Array.isArray(fallback)) {
        return (
            Array.isArray(value) &&
            value.length <= 1000 &&
            value.every((item) => fallback.length === 0 || matchesStoredShape(item, fallback[0]))
        );
    }
    if (typeof fallback === "object") {
        return (
            value !== null &&
            typeof value === "object" &&
            !Array.isArray(value) &&
            Object.entries(fallback).every(([key, expected]) =>
                matchesStoredShape((value as Record<string, unknown>)[key], expected),
            )
        );
    }
    return (
        typeof value === typeof fallback && (typeof value !== "number" || Number.isFinite(value))
    );
}

export function removeStored(storage: Storage, key: string) {
    try {
        storage.removeItem(key);
    } catch {
        // Storage can be unavailable in restricted WebViews and browser previews.
    }
}

export function browserStorage(kind: "localStorage" | "sessionStorage"): Storage | null {
    try {
        return typeof window === "undefined" ? null : window[kind];
    } catch {
        return null;
    }
}

export function writeStored(storage: Storage, key: string, value: unknown) {
    try {
        storage.setItem(key, JSON.stringify(value));
    } catch {
        // The demo remains usable when private browsing or storage quotas block persistence.
    }
}

export function createInitialState(native = false): LumoState {
    const fallback: LumoState = {
        group: EMPTY_GROUP,
        mode: null,
        demo: createDefaultDemo(),
        places: DEFAULT_PLACES,
        events: createDefaultEvents(),
        preferences: DEFAULT_PREFERENCES,
        mobile: null,
    };

    if (typeof window === "undefined") return fallback;

    const storage = browserStorage("localStorage");
    const session = browserStorage("sessionStorage");
    if (session) removeStored(session, "lumo.session");
    if (!storage) return fallback;
    removeStored(storage, "lumo.session");

    // Native builds hydrate authoritative data from Rust. Keeping group,
    // location or event snapshots in WebView storage would duplicate sensitive
    // family data outside the protected backend store.
    if (native) {
        Object.values(STORAGE_KEYS).forEach((key) => removeStored(storage, key));
        removeStored(storage, "lumo.preview-invite");
        return fallback;
    }

    const storedSchema = readStored<number>(storage, STORAGE_KEYS.schema, 0);
    if (storedSchema < 8) {
        Object.values(STORAGE_KEYS).forEach((key) => removeStored(storage, key));
        removeStored(storage, "lumo.preview-invite");
        return fallback;
    }

    const storedGroup = readStored<GroupState & { pin?: string }>(
        storage,
        STORAGE_KEYS.group,
        EMPTY_GROUP,
    );
    const { pin: _legacyPin, ...storedGroupWithoutPin } = storedGroup;
    const group =
        storedGroup.active &&
        storedGroup.name &&
        storedGroup.code &&
        (storedGroup.role === null ||
            storedGroup.role === "supervisor" ||
            storedGroup.role === "member") &&
        (storedGroup.entry === null ||
            storedGroup.entry === "created" ||
            storedGroup.entry === "joined")
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
                  supervisorPhone: storedGroup.supervisorPhone || "",
                  trackedPersonName: storedGroup.trackedPersonName || "Persona acompañada",
                  trackedPersonPhone: storedGroup.trackedPersonPhone || "",
                  role:
                      storedGroup.role ||
                      (storedGroup.entry === "joined" ? "member" : "supervisor"),
              }
            : EMPTY_GROUP;
    const storedMode = readStored<LumoState["mode"]>(storage, STORAGE_KEYS.mode, null);
    const mode =
        storedMode && ["controller", "tracker", "debug"].includes(storedMode) ? storedMode : null;
    const storedPlaces = readStored<Place[]>(storage, STORAGE_KEYS.places, fallback.places);
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
        mode: group.active ? (group.role === "member" ? "tracker" : mode) : null,
        demo: readStored(storage, STORAGE_KEYS.demo, fallback.demo),
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
        events: recentEvents(readStored(storage, STORAGE_KEYS.events, fallback.events)),
        preferences: readStored(storage, STORAGE_KEYS.preferences, fallback.preferences),
        mobile: null,
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
        case "help":
            return withEvent(
                state,
                createEvent(
                    "help",
                    `${state.group.trackedPersonName || "La persona acompañada"} necesita ayuda`,
                    "Ha solicitado que contactes cuanto antes",
                ),
                base,
            );
    }
}

function applyMobileStatus(state: LumoState, mobile: MobileRuntimeStatus): LumoState {
    // The authenticated session owns the device role; the service role is null while paused.
    const controlled = state.group.active && state.group.role === "member";
    return {
        ...state,
        mobile,
        demo: controlled
            ? {
                  ...state.demo,
                  battery: mobile.batteryPercent,
                  permission: mobile.preciseLocation === "granted" ? "granted" : "revoked",
              }
            : state.demo,
        preferences: controlled
            ? {
                  ...state.preferences,
                  // Remote tracking can belong to a replaced phone. Only local configuration
                  // proves that setup was completed on this Android installation.
                  trackerSetupComplete: mobile.controlledTrackingConfigured,
                  trackerConsents: {
                      preciseLocation: mobile.preciseLocation === "granted",
                      backgroundLocation: mobile.backgroundLocation !== "denied",
                      batteryProtection: mobile.batteryOptimizationDisabled,
                  },
              }
            : state.group.active &&
                state.group.role === "supervisor" &&
                mobile.controllerNotificationsConfigured
              ? {
                    ...state.preferences,
                    notifications: mobile.controllerNotificationsEnabled,
                }
              : state.preferences,
    };
}

export function trackerSetupStatus(
    state: LumoState,
    mobileNative: boolean,
): "pending" | "required" | "complete" {
    if (mobileNative && !state.mobile) return "pending";
    return state.preferences.trackerSetupComplete ? "complete" : "required";
}

export function reducer(state: LumoState, action: LumoAction): LumoState {
    switch (action.type) {
        case "ENTER_GROUP": {
            const {
                pin: _pin,
                invitationId: _invitationId,
                inviteToken: _inviteToken,
                ...group
            } = action.payload;
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
            return {
                ...state,
                group: EMPTY_GROUP,
                mode: null,
                mobile: null,
                preferences: DEFAULT_PREFERENCES,
                places: [],
                events: [],
                demo: createDefaultDemo(),
            };
        case "HYDRATE_BACKEND": {
            const next: LumoState = {
                ...state,
                group: action.payload.group,
                mode: action.payload.mode,
                demo: {
                    ...action.payload.demo,
                    address:
                        action.payload.demo.address ||
                        (state.group.code === action.payload.group.code &&
                        action.payload.demo.coordinates === state.demo.coordinates
                            ? state.demo.address
                            : ""),
                },
                places: action.payload.places,
                events: recentEvents(action.payload.events),
                preferences: {
                    ...state.preferences,
                    trackerSetupComplete:
                        action.payload.trackerSetupComplete ||
                        (state.group.active &&
                            action.payload.group.active &&
                            state.group.code === action.payload.group.code &&
                            state.preferences.trackerSetupComplete),
                },
            };
            return state.mobile ? applyMobileStatus(next, state.mobile) : next;
        }
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
        case "SYNC_MOBILE_STATUS": {
            return applyMobileStatus(state, action.payload);
        }
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
        case "SET_RESOLVED_ADDRESS":
            if (state.demo.coordinates !== action.payload.coordinates) return state;
            return { ...state, demo: { ...state.demo, address: action.payload.address } };
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
