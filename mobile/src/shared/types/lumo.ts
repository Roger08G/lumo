export type AppMode = "controller" | "tracker" | "debug";

export type GroupRole = "supervisor" | "member";

export type PlaceTone = "yellow" | "green" | "blue" | "pink" | "purple";

export type PlaceIcon =
    | "home"
    | "shopping"
    | "health"
    | "pin"
    | "coffee"
    | "school"
    | "work"
    | "park"
    | "favorite"
    | "activity";

export type LocationKey = "home" | "supermarket" | "medical" | "away" | "unknown";

export type EventKind = "arrival" | "departure" | "location" | "warning" | "system";

export interface GroupState {
    active: boolean;
    name: string;
    code: string;
    pin: string;
    userName: string;
    supervisorName: string;
    trackedPersonName: string;
    role: GroupRole | null;
    entry: "created" | "joined" | null;
}

export interface TripSummary {
    from: string;
    to: string;
    minutes: number;
}

export interface DemoState {
    location: LocationKey;
    placeName: string;
    statusText: string;
    sinceLabel: string;
    lastUpdatedAt: string;
    battery: number;
    connection: "online" | "offline";
    permission: "granted" | "revoked";
    accuracy: "high" | "balanced" | "low";
    delaySeconds: number;
    lastTrip: TripSummary;
}

export interface Place {
    id: string;
    name: string;
    address: string;
    coordinates: string;
    radius: number;
    kind: "home" | "shop" | "medical" | "place";
    color: PlaceTone;
    icon: PlaceIcon;
}

export interface TimelineEvent {
    id: string;
    kind: EventKind;
    title: string;
    detail: string;
    at: string;
    read: boolean;
}

export interface PreferencesState {
    notifications: boolean;
    trackerSetupComplete: boolean;
    trackerConsents: {
        preciseLocation: boolean;
        backgroundLocation: boolean;
        batteryProtection: boolean;
    };
}

export interface LumoState {
    group: GroupState;
    mode: AppMode | null;
    demo: DemoState;
    places: Place[];
    events: TimelineEvent[];
    preferences: PreferencesState;
}

export interface GroupEntryPayload {
    name: string;
    code: string;
    pin: string;
    userName: string;
    supervisorName: string;
    trackedPersonName: string;
    role: GroupRole;
    entry: "created" | "joined";
}

export type DebugScenario =
    "home" | "supermarket" | "medical" | "away" | "offline" | "permission" | "battery";

export type LumoAction =
    | { type: "ENTER_GROUP"; payload: GroupEntryPayload }
    | { type: "LEAVE_GROUP"; payload: { pin: string } }
    | { type: "SET_MODE"; payload: AppMode | null }
    | {
          type: "SET_TRACKER_CONSENT";
          payload: { key: keyof PreferencesState["trackerConsents"]; value: boolean };
      }
    | { type: "COMPLETE_TRACKER_SETUP" }
    | { type: "SET_NOTIFICATIONS"; payload: boolean }
    | { type: "FINISH_LOCATE" }
    | { type: "APPLY_SCENARIO"; payload: DebugScenario }
    | { type: "SET_BATTERY"; payload: number }
    | { type: "SET_CONNECTION"; payload: DemoState["connection"] }
    | { type: "SET_PERMISSION"; payload: DemoState["permission"] }
    | { type: "SET_ACCURACY"; payload: DemoState["accuracy"] }
    | { type: "SET_DELAY"; payload: number }
    | { type: "ADD_PLACE"; payload: Place }
    | { type: "UPDATE_PLACE"; payload: Place }
    | { type: "MARK_EVENTS_READ" }
    | { type: "PURGE_OLD_EVENTS" }
    | { type: "RESET_DEMO" };
