import { invoke } from "@tauri-apps/api/core";

import type {
    AppMode,
    BackendHydration,
    DebugScenario,
    GroupEntryPayload,
    MobileRole,
    MobileRuntimeStatus,
    Place,
} from "@shared/types/lumo.ts";

type RuntimeProfile = "controller" | "controlled" | "debug";

interface BackendSession {
    groupId: string;
    groupName: string;
    groupCode: string;
    supervisorName: string;
    supervisorPhone: string;
    trackedPersonName: string;
    trackedPersonPhone: string;
    role: "supervisor" | "member";
}

interface BackendPlace {
    id: string;
    name: string;
    address: string;
    latitude: number;
    longitude: number;
    radiusM: number;
    kind: Place["kind"];
    color: Place["color"];
    icon: Place["icon"];
}

interface BackendEvent {
    id: string;
    kind: "arrival" | "departure" | "location" | "warning" | "help" | "system";
    occurredAtMs: number;
    title: string;
    detail: string;
    readAtMs: number | null;
}

interface BackendDevice {
    precisePermission: "granted" | "revoked" | "unknown";
    backgroundPermission: "granted" | "revoked" | "unknown";
    batteryOptimizationDisabled: boolean;
    trackingEnabled: boolean;
    connectivity: "online" | "offline";
    batteryPercent: number;
    lastSeenAtMs: number | null;
    lastLocation: {
        latitude: number;
        longitude: number;
        accuracyM: number;
        capturedAtMs: number;
        batteryPercent: number;
    } | null;
    currentPlaceId: string | null;
    lastTrip: {
        from: string;
        to: string;
        durationSeconds: number;
    } | null;
}

interface BackendSnapshot {
    profile: RuntimeProfile;
    session: BackendSession | null;
    controlled: BackendDevice;
    places: BackendPlace[];
    events: BackendEvent[];
}

export interface InvitationData {
    invitationId: string;
    token: string;
    groupName: string;
    groupCode: string;
    expiresAtMs: number;
}

interface CreatePlaceInput {
    name: string;
    address: string;
    latitude: number;
    longitude: number;
    radiusM: number;
    kind: Place["kind"];
    color: Place["color"];
    icon: Place["icon"];
}

declare global {
    interface Window {
        __TAURI_INTERNALS__?: unknown;
    }
}

const isNative = () => typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
const isMobileNative = () =>
    isNative() && /Android|iPhone|iPad|iPod/i.test(window.navigator.userAgent);

async function ensureLocationPermission() {
    if (!isMobileNative()) return;
    const geolocation = await import("@tauri-apps/plugin-geolocation");
    let permissions = await geolocation.checkPermissions();
    if (permissions.location === "prompt" || permissions.location === "prompt-with-rationale") {
        permissions = await geolocation.requestPermissions(["location"]);
    }
    if (permissions.location !== "granted") {
        throw new Error("Activa el permiso de ubicación para continuar");
    }
}

function profileForMode(mode: AppMode | null): RuntimeProfile {
    if (mode === "tracker") return "controlled";
    if (mode === "debug") return "debug";
    return "controller";
}

function coordinates(latitude: number, longitude: number) {
    return `${latitude.toFixed(6)}, ${longitude.toFixed(6)}`;
}

function toPlace(place: BackendPlace): Place {
    return {
        id: place.id,
        name: place.name,
        address: place.address,
        coordinates: coordinates(place.latitude, place.longitude),
        radius: place.radiusM,
        kind: place.kind,
        color: place.color,
        icon: place.icon,
    };
}

function elapsedLabel(timestamp: number | null) {
    if (!timestamp) return "Sin datos recientes";
    const minutes = Math.max(0, Math.floor((Date.now() - timestamp) / 60_000));
    if (minutes < 1) return "Actualizado ahora";
    if (minutes < 60) return `Actualizado hace ${minutes} min`;
    return `Actualizado hace ${Math.floor(minutes / 60)} h`;
}

function hydrate(snapshot: BackendSnapshot): BackendHydration {
    const places = snapshot.places.map(toPlace);
    const activePlace = places.find((place) => place.id === snapshot.controlled.currentPlaceId);
    const role = snapshot.session?.role ?? null;
    const mode: AppMode | null = snapshot.session
        ? snapshot.profile === "controlled"
            ? "tracker"
            : snapshot.profile === "debug"
              ? "debug"
              : "controller"
        : null;
    const lastSeenAt = snapshot.controlled.lastSeenAtMs;
    const connection =
        lastSeenAt && Date.now() - lastSeenAt > 2 * 60_000
            ? "offline"
            : snapshot.controlled.connectivity;
    const location = activePlace
        ? activePlace.kind === "home"
            ? "home"
            : activePlace.kind === "shop"
              ? "supermarket"
              : activePlace.kind === "medical"
                ? "medical"
                : "away"
        : snapshot.controlled.lastLocation
          ? "away"
          : "unknown";
    const placeName =
        activePlace?.name ?? (snapshot.controlled.lastLocation ? "En trayecto" : "Sin ubicación");
    const accuracy = snapshot.controlled.lastLocation?.accuracyM ?? 999;

    return {
        group: snapshot.session
            ? {
                  active: true,
                  name: snapshot.session.groupName,
                  code: snapshot.session.groupCode,
                  userName:
                      role === "member"
                          ? snapshot.session.trackedPersonName
                          : snapshot.session.supervisorName,
                  supervisorName: snapshot.session.supervisorName,
                  supervisorPhone: snapshot.session.supervisorPhone ?? "",
                  trackedPersonName: snapshot.session.trackedPersonName,
                  trackedPersonPhone: snapshot.session.trackedPersonPhone ?? "",
                  role,
                  entry: role === "member" ? "joined" : "created",
              }
            : {
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
              },
        mode,
        demo: {
            location,
            placeName,
            statusText: activePlace
                ? `Está en ${activePlace.name.toLocaleLowerCase("es")}`
                : snapshot.controlled.lastLocation
                  ? "Está fuera de un lugar habitual"
                  : "Ubicación pendiente",
            sinceLabel: elapsedLabel(lastSeenAt),
            lastUpdatedAt: new Date(lastSeenAt ?? Date.now()).toISOString(),
            battery: snapshot.controlled.batteryPercent,
            connection,
            permission: snapshot.controlled.precisePermission === "granted" ? "granted" : "revoked",
            accuracy: accuracy <= 25 ? "high" : accuracy <= 100 ? "balanced" : "low",
            delaySeconds: lastSeenAt
                ? Math.max(0, Math.floor((Date.now() - lastSeenAt) / 1000))
                : 0,
            lastTrip: snapshot.controlled.lastTrip
                ? {
                      from: snapshot.controlled.lastTrip.from,
                      to: snapshot.controlled.lastTrip.to,
                      minutes: Math.max(
                          1,
                          Math.round(snapshot.controlled.lastTrip.durationSeconds / 60),
                      ),
                  }
                : { from: "Origen", to: placeName, minutes: 0 },
        },
        places,
        events: snapshot.events.map((event) => ({
            id: event.id,
            kind: event.kind === "help" ? "warning" : event.kind,
            title: event.title,
            detail: event.detail,
            at: new Date(event.occurredAtMs).toISOString(),
            read: event.readAtMs !== null,
        })),
        trackerSetupComplete: snapshot.controlled.trackingEnabled,
    };
}

function toCreatePlaceInput(place: Place): CreatePlaceInput {
    const [latitude, longitude] = place.coordinates.split(",").map((value) => Number(value.trim()));
    return {
        name: place.name,
        address: place.address,
        latitude,
        longitude,
        radiusM: 50,
        kind: place.kind,
        color: place.color,
        icon: place.icon,
    };
}

function readableError(error: unknown): Error {
    const value = error as { code?: string; message?: string } | null;
    const messages: Record<string, string> = {
        unauthorized: "El PIN no es correcto",
        rate_limited: "Demasiados intentos. Espera unos minutos",
        invalid_invitation: "La invitación no es válida o ya se ha utilizado",
        revision_conflict: "Los datos han cambiado en otro dispositivo. Inténtalo de nuevo",
        authentication_failed: "No se ha podido autenticar con el servidor",
        remote_unavailable: "El servidor no está disponible",
        configuration_error: "Falta completar la configuración de la API",
    };
    return new Error(
        (value?.code && messages[value.code]) ||
            value?.message ||
            "No se ha podido completar la acción",
    );
}

async function nativeInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T | null> {
    if (!isNative()) return null;
    try {
        return await invoke<T>(command, args);
    } catch (error) {
        throw readableError(error);
    }
}

function normalizedPhone(number: string) {
    const normalized = number
        .trim()
        .replace(/(?!^)\+/g, "")
        .replace(/[^\d+]/g, "");
    if (!/^\+?[0-9]{7,15}$/.test(normalized)) {
        throw new Error("No hay un número de teléfono válido configurado");
    }
    return normalized;
}

export const lumoBackend = {
    isNative,
    isMobileNative,

    async scanInvitation() {
        if (!isMobileNative()) return null;
        const scanner = await import("@tauri-apps/plugin-barcode-scanner");
        let permission = await scanner.checkPermissions();
        if (permission === "prompt") {
            permission = await scanner.requestPermissions();
        }
        if (permission !== "granted") {
            throw new Error("Activa el permiso de cámara para escanear el código QR");
        }
        const scanned = await scanner.scan({
            cameraDirection: "back",
            formats: [scanner.Format.QRCode],
        });
        let invitation: unknown;
        try {
            invitation = JSON.parse(scanned.content);
        } catch {
            throw new Error("El código QR no contiene una invitación válida");
        }
        const value = invitation as {
            version?: number;
            kind?: string;
            name?: string;
            code?: string;
            supervisorName?: string;
            trackedPersonName?: string;
            token?: string;
        };
        if (
            value.version !== 1 ||
            value.kind !== "lumo-group-invite" ||
            !value.name ||
            !value.code ||
            !value.token
        ) {
            throw new Error("El código QR no contiene una invitación válida");
        }
        return value;
    },

    async bootstrap(mode: AppMode | null) {
        const snapshot = await nativeInvoke<BackendSnapshot>("app_bootstrap", {
            profile: profileForMode(mode),
        });
        return snapshot ? hydrate(snapshot) : null;
    },

    async createGroup(payload: GroupEntryPayload) {
        const snapshot = await nativeInvoke<BackendSnapshot>("group_create", {
            input: {
                name: payload.name,
                supervisorName: payload.supervisorName,
                supervisorPhone: payload.supervisorPhone,
                trackedPersonName: payload.trackedPersonName,
                trackedPersonPhone: payload.trackedPersonPhone,
                pin: payload.pin,
            },
        });
        return snapshot ? hydrate(snapshot) : null;
    },

    async joinGroup(token: string, pin: string) {
        const verified = await nativeInvoke<{ verified: boolean }>("group_consume_invitation", {
            token,
            pin,
        });
        if (!verified) return null;
        return this.bootstrap("tracker");
    },

    async verifyPin(pin: string) {
        const verified = await nativeInvoke<{ verified: boolean }>("group_verify_pin", { pin });
        return verified?.verified ?? /^\d{6}$/.test(pin);
    },

    async createInvitation(pin: string) {
        return nativeInvoke<InvitationData>("group_create_invitation", { pin });
    },

    async leaveGroup(pin: string) {
        if (isMobileNative()) {
            const status = await this.getMobileStatus();
            if (status?.trackingEnabled && status.role) {
                await this.configureMobileTracking(status.role, false);
            }
        }
        await nativeInvoke("group_leave", { pin });
    },

    async savePlace(place: Place, editing: boolean) {
        const result = await nativeInvoke<BackendPlace>(editing ? "place_update" : "place_create", {
            ...(editing ? { id: place.id } : {}),
            input: toCreatePlaceInput(place),
        });
        return result ? toPlace(result) : place;
    },

    async deletePlace(id: string, pin: string) {
        const snapshot = await nativeInvoke<BackendSnapshot>("place_delete", { id, pin });
        return snapshot ? hydrate(snapshot) : null;
    },

    async completeTracking() {
        const result = await this.setControlledTracking(true);
        return result.snapshot;
    },

    async setControlledTracking(enabled: boolean) {
        let mobileStatus: MobileRuntimeStatus | null = null;
        if (isMobileNative()) {
            mobileStatus = enabled
                ? await this.requestMobilePermissions("controlled")
                : await this.getMobileStatus();
            if (
                enabled &&
                (!mobileStatus ||
                    mobileStatus.preciseLocation !== "granted" ||
                    mobileStatus.backgroundLocation === "denied" ||
                    !mobileStatus.locationServicesEnabled)
            ) {
                throw new Error("Completa los permisos de ubicación de Android para continuar");
            }
        } else if (enabled) {
            await ensureLocationPermission();
        }

        const backendSnapshot = await nativeInvoke<BackendSnapshot>("tracker_set_tracking", {
            input: {
                precisePermission:
                    mobileStatus?.preciseLocation === "denied" ? "revoked" : "granted",
                backgroundPermission:
                    mobileStatus?.backgroundLocation === "denied" ? "revoked" : "granted",
                batteryOptimizationDisabled: mobileStatus?.batteryOptimizationDisabled ?? true,
                enabled,
            },
        });

        try {
            if (isMobileNative()) {
                mobileStatus = await this.configureMobileTracking("controlled", enabled);
            }
        } catch (error) {
            if (enabled) {
                await nativeInvoke<BackendSnapshot>("tracker_set_tracking", {
                    input: {
                        precisePermission: "granted",
                        backgroundPermission: "granted",
                        batteryOptimizationDisabled:
                            mobileStatus?.batteryOptimizationDisabled ?? false,
                        enabled: false,
                    },
                }).catch(() => undefined);
            }
            throw error;
        }

        return {
            status: mobileStatus,
            snapshot: backendSnapshot ? hydrate(backendSnapshot) : null,
        };
    },

    async getMobileStatus() {
        if (!isMobileNative()) return null;
        return nativeInvoke<MobileRuntimeStatus>("mobile_get_status");
    },

    async requestMobilePermissions(role: MobileRole) {
        if (!isMobileNative()) return null;
        return nativeInvoke<MobileRuntimeStatus>("mobile_request_permissions", { role });
    },

    async configureMobileTracking(role: MobileRole, enabled: boolean) {
        if (!isMobileNative()) return null;
        return nativeInvoke<MobileRuntimeStatus>("mobile_configure_tracking", {
            role,
            enabled,
            intervalSeconds: 30,
        });
    },

    async openBatterySettings() {
        if (!isMobileNative()) return false;
        await nativeInvoke("mobile_open_battery_settings");
        return true;
    },

    async openPhoneDialer(number: string) {
        const phone = normalizedPhone(number);
        if (!isNative()) return false;
        await nativeInvoke("mobile_open_phone_dialer", { number: phone });
        return true;
    },

    async showNotification(
        title: string,
        body: string,
        options: { id?: string; urgent?: boolean } = {},
    ) {
        if (!isMobileNative()) return false;
        await nativeInvoke("mobile_show_notification", {
            id: options.id,
            title,
            body,
            urgent: options.urgent ?? false,
        });
        return true;
    },

    async requestLocation() {
        return nativeInvoke<{ commandId: string; status: string }>("controller_request_location");
    },

    async processPending() {
        return nativeInvoke<{ processed: number }>("tracker_process_pending");
    },

    async captureLocation() {
        if (!isMobileNative()) return null;
        await ensureLocationPermission();
        const geolocation = await import("@tauri-apps/plugin-geolocation");
        return geolocation.getCurrentPosition({
            enableHighAccuracy: true,
            maximumAge: 30_000,
            timeout: 15_000,
        });
    },

    async reportLocation(
        latitude: number,
        longitude: number,
        accuracyM: number,
        batteryPercent: number,
    ) {
        const snapshot = await nativeInvoke<BackendSnapshot>("tracker_report_location", {
            input: {
                latitude,
                longitude,
                accuracyM,
                batteryPercent,
                capturedAtMs: Date.now(),
            },
        });
        return snapshot ? hydrate(snapshot) : null;
    },

    async sendHelp() {
        const snapshot = await nativeInvoke<BackendSnapshot>("tracker_send_help");
        return snapshot ? hydrate(snapshot) : null;
    },

    async markEventsRead() {
        const snapshot = await nativeInvoke<BackendSnapshot>("events_mark_read");
        return snapshot ? hydrate(snapshot) : null;
    },

    async applyDebugScenario(scenario: DebugScenario) {
        const snapshot = await nativeInvoke<BackendSnapshot>("debug_apply_scenario", { scenario });
        return snapshot ? hydrate(snapshot) : null;
    },
};

export type LumoBackend = typeof lumoBackend;
