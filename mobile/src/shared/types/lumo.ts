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

export type EventKind = "arrival" | "departure" | "location" | "warning" | "help" | "system";

export interface GroupState {
    active: boolean;
    name: string;
    code: string;
    userName: string;
    supervisorName: string;
    supervisorPhone: string;
    trackedPersonName: string;
    trackedPersonPhone: string;
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
    coordinates: string | null;
    address: string;
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

export type MobileRole = "controller" | "controlled";

export interface MobileRuntimeStatus {
    platform: "android";
    trackingEnabled: boolean;
    controlledTrackingMayAutoRecover: boolean;
    role: MobileRole | null;
    preciseLocation: "granted" | "denied";
    backgroundLocation: "granted" | "denied" | "notRequired";
    notifications: "granted" | "denied";
    batteryOptimizationDisabled: boolean;
    batteryPercent: number;
    locationServicesEnabled: boolean;
    controllerNotificationsConfigured: boolean;
    controllerNotificationsEnabled: boolean;
}

export interface LumoState {
    group: GroupState;
    mode: AppMode | null;
    demo: DemoState;
    places: Place[];
    events: TimelineEvent[];
    preferences: PreferencesState;
    mobile: MobileRuntimeStatus | null;
}

export interface GroupEntryPayload {
    name: string;
    code: string;
    pin: string;
    userName: string;
    supervisorName: string;
    supervisorPhone: string;
    trackedPersonName: string;
    trackedPersonPhone: string;
    role: GroupRole;
    entry: "created" | "joined";
    invitationId?: string;
    inviteToken?: string;
}

export interface BackendHydration {
    group: GroupState;
    mode: AppMode | null;
    demo: DemoState;
    places: Place[];
    events: TimelineEvent[];
    trackerSetupComplete: boolean;
}

export type DebugScenario =
    "home" | "supermarket" | "medical" | "away" | "offline" | "permission" | "battery";

export type LumoAction =
    | { type: "ENTER_GROUP"; payload: GroupEntryPayload }
    | { type: "LEAVE_GROUP" }
    | { type: "HYDRATE_BACKEND"; payload: BackendHydration }
    | { type: "SET_MODE"; payload: AppMode | null }
    | {
          type: "SET_TRACKER_CONSENT";
          payload: { key: keyof PreferencesState["trackerConsents"]; value: boolean };
      }
    | { type: "COMPLETE_TRACKER_SETUP" }
    | { type: "SET_NOTIFICATIONS"; payload: boolean }
    | { type: "SYNC_MOBILE_STATUS"; payload: MobileRuntimeStatus }
    | { type: "FINISH_LOCATE" }
    | { type: "APPLY_SCENARIO"; payload: DebugScenario }
    | { type: "SET_BATTERY"; payload: number }
    | { type: "SET_CONNECTION"; payload: DemoState["connection"] }
    | { type: "SET_PERMISSION"; payload: DemoState["permission"] }
    | { type: "SET_ACCURACY"; payload: DemoState["accuracy"] }
    | { type: "SET_DELAY"; payload: number }
    | { type: "SET_RESOLVED_ADDRESS"; payload: { coordinates: string; address: string } }
    | { type: "ADD_PLACE"; payload: Place }
    | { type: "UPDATE_PLACE"; payload: Place }
    | { type: "DELETE_PLACE"; payload: { id: string } }
    | { type: "MARK_EVENTS_READ" }
    | { type: "PURGE_OLD_EVENTS" }
    | { type: "RESET_DEMO" };
