import { useEffect, useMemo, useState, type FormEvent } from "react";
import { css, keyframes } from "@emotion/react";
import {
    FiActivity,
    FiAlertTriangle,
    FiBattery,
    FiBell,
    FiBookOpen,
    FiBriefcase,
    FiCheck,
    FiChevronLeft,
    FiChevronRight,
    FiClock,
    FiCoffee,
    FiCrosshair,
    FiHeart,
    FiHome,
    FiLogOut,
    FiMapPin,
    FiNavigation,
    FiPhone,
    FiPlus,
    FiSettings,
    FiShield,
    FiShoppingBag,
    FiSliders,
    FiStar,
    FiSun,
    FiTrash2,
    FiUser,
    FiUserPlus,
    FiWifi,
} from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import {
    GroupSecurityModal,
    type GroupSecurityAction,
} from "@modules/groups/components/GroupSecurityModal.tsx";
import { ProtectedActionModal } from "@modules/groups/components/ProtectedActionModal.tsx";
import { BottomNavigation, type ControllerTab } from "@shared/components/BottomNavigation.tsx";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { StepProgress } from "@shared/components/StepProgress.tsx";
import { TopSheet } from "@shared/components/TopSheet.tsx";
import { Button, Field, IconButton, Modal, Pill, Toast, Toggle } from "@shared/components/ui.tsx";
import { PLACE_PALETTE, PLACE_TONES, randomPlaceTone } from "@shared/styles/placePalette.ts";
import { surface } from "@shared/styles/surfaces.ts";
import type { EventKind, Place, PlaceIcon, PlaceTone, TimelineEvent } from "@shared/types/lumo.ts";
import { formatCoordinates, parseCoordinates } from "@shared/utils/coordinates.ts";
import { formatClock, formatRelative, greeting } from "@shared/utils/format.ts";

const pulse = keyframes({
    "0%": { boxShadow: "0 0 0 0 rgba(104,66,166,.28)" },
    "70%": { boxShadow: "0 0 0 14px rgba(104,66,166,0)" },
    "100%": { boxShadow: "0 0 0 0 rgba(104,66,166,0)" },
});

const routeFlow = keyframes({
    from: { backgroundPositionX: "0" },
    to: { backgroundPositionX: "24px" },
});

const routeTravel = keyframes({
    "0%": { left: 2, opacity: 0, transform: "translateY(-50%) scale(.8)" },
    "15%": { opacity: 1 },
    "85%": { opacity: 1 },
    "100%": {
        left: "calc(100% - 18px)",
        opacity: 0,
        transform: "translateY(-50%) scale(1)",
    },
});

const sectionTitle = css({
    color: "var(--lumo-text)",
    fontSize: 17,
    letterSpacing: "-.02em",
});

interface ToastState {
    title: string;
    detail?: string;
}

type ControllerProtectedAction =
    { kind: "change-mode" } | { kind: "delete-place"; place: Place } | null;

function TripRoute({ from, to }: { from: string; to: string }) {
    return (
        <div
            aria-label={`Trayecto desde ${from} hasta ${to}`}
            css={css({
                display: "grid",
                gridTemplateColumns: "minmax(0, 96px) minmax(36px, 1fr) minmax(0, 112px)",
                alignItems: "center",
                gap: 6,
                marginTop: 13,
            })}
        >
            <strong
                title={from}
                css={css({
                    overflow: "hidden",
                    maxWidth: 96,
                    color: "var(--lumo-text)",
                    fontSize: 15,
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                })}
            >
                {from}
            </strong>
            <span
                aria-hidden="true"
                css={css({
                    position: "relative",
                    height: 26,
                    display: "block",
                    "&::before": {
                        content: '""',
                        position: "absolute",
                        top: "50%",
                        right: 5,
                        left: 5,
                        height: 2,
                        borderRadius: 999,
                        background:
                            "repeating-linear-gradient(90deg, var(--lumo-accent) 0 7px, transparent 7px 12px)",
                        animation: `${routeFlow} 1.15s linear infinite`,
                    },
                })}
            >
                <span
                    css={css({
                        position: "absolute",
                        top: "50%",
                        left: 2,
                        zIndex: 1,
                        width: 18,
                        height: 18,
                        display: "grid",
                        placeItems: "center",
                        borderRadius: 8,
                        color: "#fff",
                        background: "var(--lumo-primary)",
                        boxShadow: "0 4px 10px rgba(104,66,166,.24)",
                        animation: `${routeTravel} 2.4s ease-in-out infinite`,
                    })}
                >
                    <FiUser size={12} />
                </span>
            </span>
            <strong
                title={to}
                css={css({
                    overflow: "hidden",
                    maxWidth: 122,
                    color: "var(--lumo-primary)",
                    fontSize: 15,
                    textAlign: "right",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                })}
            >
                {to}
            </strong>
        </div>
    );
}

function StatusIcon({ warning }: { warning: boolean }) {
    return (
        <span
            css={css({
                width: 38,
                height: 38,
                display: "grid",
                placeItems: "center",
                borderRadius: 13,
                color: warning ? "var(--lumo-warning)" : "var(--lumo-success)",
                background: warning ? "var(--lumo-warning-soft)" : "var(--lumo-success-soft)",
            })}
        >
            {warning ? <FiAlertTriangle size={19} /> : <FiHome size={19} />}
        </span>
    );
}

function MockMap() {
    const { state } = useLumo();
    const warning = state.demo.connection === "offline" || state.demo.permission === "revoked";

    return (
        <div
            role="img"
            aria-label={`Mapa simulado. Última ubicación: ${state.demo.placeName}`}
            css={css({
                position: "relative",
                height: 188,
                overflow: "hidden",
                border: "1px solid #dfd9cf",
                borderRadius: 22,
                background:
                    "linear-gradient(28deg, transparent 47%, rgba(255,255,255,.92) 47% 54%, transparent 54%), linear-gradient(112deg, transparent 43%, rgba(255,255,255,.82) 43% 51%, transparent 51%), #e9e5db",
            })}
        >
            <span
                aria-hidden="true"
                css={css({
                    position: "absolute",
                    top: 18,
                    left: 18,
                    width: 62,
                    height: 46,
                    borderRadius: "14px 7px 10px 6px",
                    background: "#d6dfcd",
                    transform: "rotate(-6deg)",
                })}
            />
            <span
                aria-hidden="true"
                css={css({
                    position: "absolute",
                    right: 24,
                    bottom: 16,
                    width: 86,
                    height: 54,
                    borderRadius: "12px 20px 9px 16px",
                    background: "#d4deca",
                    transform: "rotate(7deg)",
                })}
            />
            <span
                aria-hidden="true"
                css={css({
                    position: "absolute",
                    left: 21,
                    bottom: 31,
                    width: 70,
                    height: 38,
                    borderRadius: 10,
                    background: "#ddd2c6",
                })}
            />
            <div
                css={css({
                    position: "absolute",
                    top: "48%",
                    left: "53%",
                    display: "grid",
                    justifyItems: "center",
                    gap: 6,
                    transform: "translate(-50%, -50%)",
                })}
            >
                <span
                    css={css({
                        width: 43,
                        height: 43,
                        display: "grid",
                        placeItems: "center",
                        border: "4px solid #fff",
                        borderRadius: "50% 50% 50% 10%",
                        color: "#fff",
                        background: warning ? "var(--lumo-warning)" : "var(--lumo-primary)",
                        transform: "rotate(-45deg)",
                        boxShadow: "0 8px 18px rgba(54,38,69,.24)",
                        animation: warning ? undefined : `${pulse} 2.4s ease-out infinite`,
                        "& > svg": { transform: "rotate(45deg)" },
                    })}
                >
                    {state.demo.location === "home" ? <FiHome size={18} /> : <FiMapPin size={18} />}
                </span>
                <span
                    css={css({
                        padding: "5px 9px",
                        borderRadius: 9,
                        color: "var(--lumo-text)",
                        background: "rgba(255,255,255,.94)",
                        boxShadow: "0 4px 12px rgba(47,38,57,.12)",
                        fontSize: 10,
                    })}
                >
                    {state.demo.placeName}
                </span>
            </div>
            <span
                css={css({
                    position: "absolute",
                    right: 11,
                    top: 11,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 5,
                    padding: "6px 9px",
                    borderRadius: 10,
                    color: "var(--lumo-text-secondary)",
                    background: "rgba(255,255,255,.88)",
                    fontSize: 9,
                })}
            >
                <FiClock /> {formatRelative(state.demo.lastUpdatedAt)}
            </span>
            {warning && (
                <div
                    css={css({
                        position: "absolute",
                        inset: 0,
                        display: "grid",
                        placeItems: "end center",
                        paddingBottom: 12,
                        background: "linear-gradient(transparent 45%, rgba(248,245,239,.82))",
                    })}
                >
                    <Pill tone="amber">
                        <FiAlertTriangle /> Última posición conocida
                    </Pill>
                </div>
            )}
        </div>
    );
}

const eventIcons: Record<EventKind, IconType> = {
    arrival: FiMapPin,
    departure: FiNavigation,
    location: FiCrosshair,
    warning: FiAlertTriangle,
    system: FiShield,
};

function Timeline({ events, compact = false }: { events: TimelineEvent[]; compact?: boolean }) {
    if (events.length === 0) {
        return (
            <p
                css={css({
                    padding: "18px 4px",
                    color: "var(--lumo-text-secondary)",
                    fontSize: 12,
                    lineHeight: 1.5,
                    textAlign: "center",
                })}
            >
                No hay actividad en las últimas 24 horas.
            </p>
        );
    }

    return (
        <div css={css({ display: "grid" })}>
            {events.map((event, index) => {
                const Icon = eventIcons[event.kind];
                const warning = event.kind === "warning";
                return (
                    <article
                        key={event.id}
                        css={css({
                            position: "relative",
                            display: "grid",
                            gridTemplateColumns: "40px 1fr auto",
                            gap: 11,
                            padding: compact ? "10px 0" : "13px 0",
                            borderBottom:
                                index === events.length - 1 ? 0 : "1px solid var(--lumo-border)",
                        })}
                    >
                        <span
                            css={css({
                                width: 40,
                                height: 40,
                                display: "grid",
                                placeItems: "center",
                                borderRadius: 13,
                                color: warning ? "var(--lumo-warning)" : "var(--lumo-primary)",
                                background: warning
                                    ? "var(--lumo-warning-soft)"
                                    : "var(--lumo-lavender)",
                            })}
                        >
                            <Icon size={17} aria-hidden="true" />
                        </span>
                        <span
                            css={css({
                                minWidth: 0,
                                display: "grid",
                                alignContent: "center",
                                gap: 4,
                            })}
                        >
                            <strong
                                css={css({
                                    color: "var(--lumo-text)",
                                    fontSize: 12,
                                    fontWeight: 500,
                                })}
                            >
                                {event.title}
                            </strong>
                            <span
                                css={css({
                                    overflow: "hidden",
                                    color: "var(--lumo-text-muted)",
                                    fontSize: 10,
                                    lineHeight: 1.4,
                                    textOverflow: "ellipsis",
                                    whiteSpace: compact ? "nowrap" : "normal",
                                })}
                            >
                                {event.detail}
                            </span>
                        </span>
                        <time
                            dateTime={event.at}
                            css={css({
                                color: "var(--lumo-text-muted)",
                                fontSize: 9,
                                paddingTop: 3,
                            })}
                        >
                            {formatClock(event.at)}
                        </time>
                    </article>
                );
            })}
        </div>
    );
}

function HomeView({
    locating,
    onLocate,
    onCall,
    onShowActivity,
}: {
    locating: boolean;
    onLocate: () => void;
    onCall: () => void;
    onShowActivity: () => void;
}) {
    const { state } = useLumo();
    const warning = state.demo.connection === "offline" || state.demo.permission === "revoked";

    return (
        <div css={css({ display: "grid", gap: 16 })}>
            <section
                css={css(surface, {
                    padding: 17,
                    background: warning
                        ? "linear-gradient(145deg, #fff, var(--lumo-warning-soft))"
                        : "linear-gradient(145deg, #fff, #f8f4fd)",
                })}
            >
                <div css={css({ display: "flex", alignItems: "flex-start", gap: 11 })}>
                    <StatusIcon warning={warning} />
                    <div css={css({ minWidth: 0, display: "grid", gap: 4, flex: 1 })}>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10 })}>
                            ESTADO DE {state.group.trackedPersonName.toUpperCase()}
                        </span>
                        <h2 css={css({ fontSize: 20, lineHeight: 1.2, letterSpacing: "-.03em" })}>
                            {state.demo.statusText}
                        </h2>
                        <p css={css({ color: "var(--lumo-text-secondary)", fontSize: 11 })}>
                            {state.demo.sinceLabel}
                        </p>
                    </div>
                    <Pill tone={warning ? "amber" : "green"}>
                        {warning ? "Revisar" : "Todo bien"}
                    </Pill>
                </div>
                <div
                    css={css({
                        display: "grid",
                        gridTemplateColumns: "1fr 1fr 1fr",
                        gap: 7,
                        marginTop: 16,
                        paddingTop: 14,
                        borderTop: "1px solid var(--lumo-border)",
                    })}
                >
                    {[
                        {
                            icon: FiClock,
                            label: "Actualizado",
                            value: formatRelative(state.demo.lastUpdatedAt),
                        },
                        { icon: FiBattery, label: "Batería", value: `${state.demo.battery} %` },
                        {
                            icon: FiWifi,
                            label: "Conexión",
                            value: state.demo.connection === "online" ? "En línea" : "Sin señal",
                        },
                    ].map((item) => (
                        <span key={item.label} css={css({ minWidth: 0, display: "grid", gap: 4 })}>
                            <span
                                css={css({
                                    display: "flex",
                                    alignItems: "center",
                                    gap: 4,
                                    color: "var(--lumo-text-muted)",
                                    fontSize: 9,
                                })}
                            >
                                <item.icon size={12} /> {item.label}
                            </span>
                            <strong
                                css={css({
                                    overflow: "hidden",
                                    color: "var(--lumo-text)",
                                    fontSize: 11,
                                    textOverflow: "ellipsis",
                                    whiteSpace: "nowrap",
                                })}
                            >
                                {item.value}
                            </strong>
                        </span>
                    ))}
                </div>
            </section>

            <MockMap />

            <div css={css({ display: "grid", gridTemplateColumns: "1fr auto", gap: 9 })}>
                <Button fullWidth icon={FiCrosshair} loading={locating} onClick={onLocate}>
                    {locating ? "Localizando…" : "Localizar ahora"}
                </Button>
                <Button
                    variant="secondary"
                    icon={FiPhone}
                    aria-label={`Llamar a ${state.group.trackedPersonName}`}
                    onClick={onCall}
                >
                    Llamar
                </Button>
            </div>

            <section css={css(surface, { padding: 17 })}>
                <div
                    css={css({
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                    })}
                >
                    <div css={css({ display: "grid", gap: 4 })}>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10 })}>
                            ÚLTIMO TRAYECTO
                        </span>
                        <h3 css={sectionTitle}>Ruta completada</h3>
                    </div>
                    <Pill>
                        <FiClock /> {state.demo.lastTrip.minutes} min
                    </Pill>
                </div>
                <TripRoute from={state.demo.lastTrip.from} to={state.demo.lastTrip.to} />
            </section>

            <section css={css(surface, { padding: "17px 17px 5px" })}>
                <header
                    css={css({
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                        marginBottom: 3,
                    })}
                >
                    <h3 css={sectionTitle}>Actividad reciente</h3>
                    <button
                        type="button"
                        onClick={onShowActivity}
                        css={css({
                            minHeight: 36,
                            display: "inline-flex",
                            alignItems: "center",
                            gap: 4,
                            border: 0,
                            color: "var(--lumo-primary)",
                            background: "transparent",
                            cursor: "pointer",
                            fontSize: 11,
                        })}
                    >
                        Ver todo <FiChevronRight />
                    </button>
                </header>
                <Timeline events={state.events.slice(0, 3)} compact />
            </section>
        </div>
    );
}

function ActivityView() {
    const { state } = useLumo();

    return (
        <section css={css({ display: "grid", gap: 16 })}>
            <div css={css({ display: "grid", gap: 5 })}>
                <Pill>{state.events.length} eventos · 24 h</Pill>
                <h2 css={css({ fontSize: 24, letterSpacing: "-.035em" })}>Últimas 24 horas</h2>
                <p
                    css={css({
                        color: "var(--lumo-text-secondary)",
                        fontSize: 12,
                        lineHeight: 1.5,
                    })}
                >
                    Las llegadas, salidas y avisos se eliminan automáticamente al cumplir 24 horas.
                </p>
            </div>
            {state.events.length > 0 ? (
                <div css={css(surface, { padding: "4px 17px" })}>
                    <Timeline events={state.events} />
                </div>
            ) : (
                <div
                    css={css(surface, {
                        padding: "26px 18px",
                        color: "var(--lumo-text-secondary)",
                        fontSize: 12,
                        textAlign: "center",
                    })}
                >
                    Hoy todavía no hay actividad.
                </div>
            )}
        </section>
    );
}

const PLACE_ICON_OPTIONS: Array<{ key: PlaceIcon; label: string; icon: IconType }> = [
    { key: "home", label: "Casa", icon: FiHome },
    { key: "shopping", label: "Compras", icon: FiShoppingBag },
    { key: "health", label: "Salud", icon: FiHeart },
    { key: "pin", label: "Lugar", icon: FiMapPin },
    { key: "coffee", label: "Cafetería", icon: FiCoffee },
    { key: "school", label: "Estudios", icon: FiBookOpen },
    { key: "work", label: "Trabajo", icon: FiBriefcase },
    { key: "park", label: "Parque", icon: FiSun },
    { key: "favorite", label: "Favorito", icon: FiStar },
    { key: "activity", label: "Actividad", icon: FiActivity },
];

const placeIcons = Object.fromEntries(
    PLACE_ICON_OPTIONS.map((option) => [option.key, option.icon]),
) as Record<PlaceIcon, IconType>;

function PlacesView({ onAdd, onEdit }: { onAdd: () => void; onEdit: (place: Place) => void }) {
    const { state } = useLumo();

    return (
        <section css={css({ display: "grid", gap: 16 })}>
            <div
                css={css({
                    display: "flex",
                    alignItems: "flex-end",
                    justifyContent: "space-between",
                    gap: 14,
                })}
            >
                <div css={css({ display: "grid", gap: 5 })}>
                    <Pill>{state.places.length} zonas activas</Pill>
                    <h2 css={css({ fontSize: 24, letterSpacing: "-.035em" })}>
                        Lugares habituales
                    </h2>
                </div>
                <IconButton label="Añadir lugar" icon={FiPlus} onClick={onAdd} />
            </div>
            <p css={css({ color: "var(--lumo-text-secondary)", fontSize: 12, lineHeight: 1.5 })}>
                Recibirás un aviso simulado cuando {state.group.trackedPersonName} entre o salga de
                estas zonas. Pulsa una para editarla.
            </p>
            <div css={css({ display: "grid", gap: 10 })}>
                {state.places.map((place) => {
                    const Icon = placeIcons[place.icon] ?? FiMapPin;
                    const palette = PLACE_PALETTE[place.color] ?? PLACE_PALETTE.purple;
                    return (
                        <button
                            type="button"
                            key={place.id}
                            aria-label={`Editar ${place.name}`}
                            onClick={() => onEdit(place)}
                            css={css(surface, {
                                width: "100%",
                                display: "grid",
                                gridTemplateColumns: "48px 1fr auto",
                                alignItems: "center",
                                gap: 12,
                                padding: 14,
                                borderLeft: `3px solid ${palette.foreground}36`,
                                color: "inherit",
                                textAlign: "left",
                                cursor: "pointer",
                                transition: "transform .2s ease, box-shadow .2s ease",
                                "&:hover": {
                                    transform: "translateY(-1px)",
                                    boxShadow: "0 12px 30px rgba(47,38,57,.075)",
                                },
                                "&:focus-visible": {
                                    borderColor: palette.foreground,
                                },
                            })}
                        >
                            <span
                                css={css({
                                    width: 48,
                                    height: 48,
                                    display: "grid",
                                    placeItems: "center",
                                    borderRadius: 16,
                                    color: palette.foreground,
                                    background: palette.background,
                                })}
                            >
                                <Icon size={21} />
                            </span>
                            <div css={css({ minWidth: 0, display: "grid", gap: 4 })}>
                                <strong css={css({ fontSize: 14 })}>{place.name}</strong>
                                <span
                                    css={css({
                                        overflow: "hidden",
                                        color: "var(--lumo-text-muted)",
                                        fontSize: 10,
                                        textOverflow: "ellipsis",
                                        whiteSpace: "nowrap",
                                    })}
                                >
                                    {place.address}
                                </span>
                                <span
                                    css={css({ color: "var(--lumo-text-secondary)", fontSize: 10 })}
                                >
                                    Radio de {place.radius} m
                                </span>
                            </div>
                            <FiChevronRight color="var(--lumo-text-muted)" />
                        </button>
                    );
                })}
            </div>
            <Button variant="secondary" fullWidth icon={FiPlus} onClick={onAdd}>
                Añadir un lugar
            </Button>
        </section>
    );
}

function SettingsView({
    onToast,
    onInvite,
    onChangeMode,
    onLeave,
}: {
    onToast: (toast: ToastState) => void;
    onInvite: () => void;
    onChangeMode: () => void;
    onLeave: () => void;
}) {
    const { state, dispatch, backend } = useLumo();

    return (
        <section css={css({ display: "grid", gap: 16 })}>
            <div css={css({ display: "grid", gap: 5 })}>
                <Pill>Cuenta de supervisor</Pill>
                <h2 css={css({ fontSize: 24, letterSpacing: "-.035em" })}>
                    Hola, {state.group.userName}
                </h2>
                <p css={css({ color: "var(--lumo-text-secondary)", fontSize: 12 })}>
                    Grupo vinculado: {state.group.name}
                </p>
            </div>

            <article css={css(surface, { display: "grid", padding: "5px 16px" })}>
                <Toggle
                    label="Avisos familiares"
                    description="Recibir llegadas, salidas y avisos de ayuda"
                    checked={state.preferences.notifications}
                    onChange={async (checked) => {
                        try {
                            if (!checked) {
                                await backend.configureMobileTracking("controller", false);
                                dispatch({ type: "SET_NOTIFICATIONS", payload: false });
                                onToast({ title: "Avisos pausados" });
                                return;
                            }
                            const status = await backend.requestMobilePermissions("controller");
                            if (status) {
                                dispatch({ type: "SYNC_MOBILE_STATUS", payload: status });
                            }
                            if (status && status.notifications !== "granted") {
                                dispatch({ type: "SET_NOTIFICATIONS", payload: false });
                                onToast({
                                    title: "Permiso pendiente",
                                    detail: "Activa las notificaciones en los ajustes de Android",
                                });
                                return;
                            }
                            const trackingStatus = await backend.configureMobileTracking(
                                "controller",
                                true,
                            );
                            if (trackingStatus) {
                                dispatch({
                                    type: "SYNC_MOBILE_STATUS",
                                    payload: trackingStatus,
                                });
                            }
                            dispatch({ type: "SET_NOTIFICATIONS", payload: true });
                            onToast({ title: "Avisos activados" });
                        } catch (requestError) {
                            dispatch({ type: "SET_NOTIFICATIONS", payload: false });
                            onToast({
                                title: "No se han podido activar",
                                detail:
                                    requestError instanceof Error
                                        ? requestError.message
                                        : "Revisa los ajustes de Android",
                            });
                        }
                    }}
                />
                <div css={css({ height: 1, background: "var(--lumo-border)" })} />
                <Toggle
                    label="Conexión del tracker"
                    description="Estado comunicado por el teléfono acompañado"
                    checked={state.demo.connection === "online"}
                    disabled
                    onChange={() => undefined}
                />
            </article>

            <article css={css(surface, { display: "grid", gap: 9, padding: 16 })}>
                <div css={css({ display: "flex", alignItems: "center", gap: 10 })}>
                    <span
                        css={css({
                            width: 40,
                            height: 40,
                            display: "grid",
                            placeItems: "center",
                            borderRadius: 13,
                            color: "var(--lumo-primary)",
                            background: "var(--lumo-lavender)",
                        })}
                    >
                        <FiSliders />
                    </span>
                    <div css={css({ display: "grid", gap: 2 })}>
                        <strong css={css({ fontSize: 14 })}>Cambiar la experiencia</strong>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10 })}>
                            Elige la vista disponible en este teléfono
                        </span>
                    </div>
                </div>
                <Button variant="secondary" fullWidth icon={FiSettings} onClick={onChangeMode}>
                    Elegir otro modo
                </Button>
            </article>

            <article css={css(surface, { display: "grid", gap: 11, padding: 16 })}>
                <div css={css({ display: "flex", alignItems: "center", gap: 10 })}>
                    <span
                        css={css({
                            width: 40,
                            height: 40,
                            display: "grid",
                            placeItems: "center",
                            borderRadius: 13,
                            color: "var(--lumo-primary)",
                            background: "var(--lumo-lavender)",
                        })}
                    >
                        <FiUserPlus />
                    </span>
                    <div css={css({ display: "grid", gap: 2 })}>
                        <strong css={css({ fontSize: 14 })}>Miembros e invitaciones</strong>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10 })}>
                            El PIN protege los datos de acceso al grupo
                        </span>
                    </div>
                </div>
                <Button variant="secondary" fullWidth icon={FiUserPlus} onClick={onInvite}>
                    Invitar a un miembro
                </Button>
            </article>

            <Button variant="danger" fullWidth icon={FiLogOut} onClick={onLeave}>
                Desvincular este teléfono
            </Button>
            <p
                css={css({
                    color: "var(--lumo-text-muted)",
                    textAlign: "center",
                    fontSize: 10,
                    lineHeight: 1.5,
                })}
            >
                Al desvincularlo se conserva el escenario local para que puedas retomarlo después.
            </p>
        </section>
    );
}

function PlaceModal({
    open,
    place,
    onClose,
    onSaved,
    onDelete,
}: {
    open: boolean;
    place: Place | null;
    onClose: () => void;
    onSaved: (editing: boolean) => void;
    onDelete: (place: Place) => void;
}) {
    const { state, dispatch, backend } = useLumo();
    const [step, setStep] = useState<0 | 1>(0);
    const [name, setName] = useState("");
    const [address, setAddress] = useState("");
    const [coordinates, setCoordinates] = useState("");
    const [icon, setIcon] = useState<PlaceIcon>("pin");
    const [color, setColor] = useState<PlaceTone>("purple");
    const [error, setError] = useState("");
    const [saving, setSaving] = useState(false);
    const editing = Boolean(place);

    useEffect(() => {
        if (!open) return;
        setStep(0);
        setName(place?.name ?? "");
        setAddress(place?.address ?? "");
        setCoordinates(place?.coordinates ?? "");
        setIcon(place?.icon ?? "pin");
        setColor(place?.color ?? randomPlaceTone(state.places[state.places.length - 1]?.color));
        setError("");
        setSaving(false);
    }, [open, place]);

    const kindForIcon = (selectedIcon: PlaceIcon): Place["kind"] => {
        if (selectedIcon === "home") return "home";
        if (selectedIcon === "shopping") return "shop";
        if (selectedIcon === "health") return "medical";
        return "place";
    };

    const save = async (event: FormEvent) => {
        event.preventDefault();
        if (step === 0) {
            if (name.trim().length < 2 || address.trim().length < 4) {
                setError("Completa el nombre y la dirección exacta");
                return;
            }
            const parsed = parseCoordinates(coordinates);
            if (!parsed) {
                setError("Introduce una latitud y una longitud válidas");
                return;
            }
            setCoordinates(formatCoordinates(parsed));
            setError("");
            setStep(1);
            return;
        }

        const nextPlace: Place = {
            id: place?.id ?? `${Date.now()}`,
            name: name.trim(),
            address: address.trim(),
            coordinates: coordinates.trim(),
            radius: place?.radius ?? 50,
            kind: kindForIcon(icon),
            color,
            icon,
        };

        try {
            setSaving(true);
            const savedPlace = await backend.savePlace(nextPlace, editing);
            dispatch({
                type: editing ? "UPDATE_PLACE" : "ADD_PLACE",
                payload: savedPlace,
            });
            onClose();
            onSaved(editing);
        } catch (requestError) {
            setError(
                requestError instanceof Error
                    ? requestError.message
                    : "No se ha podido guardar el lugar",
            );
            setSaving(false);
        }
    };

    const SelectedIcon = placeIcons[icon];
    const selectedPalette = PLACE_PALETTE[color];

    return (
        <Modal
            open={open}
            onClose={onClose}
            eyebrow={`Paso ${step + 1} de 2`}
            title={step === 0 ? (editing ? "Editar los datos" : "Nuevo lugar") : "Elige su estilo"}
        >
            <form onSubmit={save} css={css({ display: "grid", gap: 15 })}>
                <StepProgress current={step} total={2} variant="bars" />

                {step === 0 ? (
                    <>
                        <Field
                            autoFocus
                            label="Nombre del lugar"
                            placeholder="Nombre del lugar"
                            icon={FiMapPin}
                            value={name}
                            onChange={(event) => {
                                setName(event.target.value);
                                setError("");
                            }}
                        />
                        <Field
                            label="Dirección exacta"
                            placeholder="Dirección exacta"
                            icon={FiHome}
                            value={address}
                            onChange={(event) => {
                                setAddress(event.target.value);
                                setError("");
                            }}
                        />
                        <Field
                            label="Coordenadas"
                            placeholder="Latitud, longitud"
                            icon={FiCrosshair}
                            inputMode="decimal"
                            autoComplete="off"
                            autoCapitalize="none"
                            spellCheck={false}
                            value={coordinates}
                            onChange={(event) => {
                                setCoordinates(event.target.value);
                                setError("");
                            }}
                        />
                    </>
                ) : (
                    <>
                        <div
                            css={css({
                                display: "flex",
                                alignItems: "center",
                                gap: 12,
                                padding: 13,
                                border: `1px solid ${selectedPalette.foreground}35`,
                                borderRadius: 18,
                                background: selectedPalette.background,
                            })}
                        >
                            <span
                                css={css({
                                    width: 48,
                                    height: 48,
                                    display: "grid",
                                    placeItems: "center",
                                    borderRadius: 16,
                                    color: selectedPalette.foreground,
                                    background: "rgba(255,255,255,.68)",
                                })}
                            >
                                <SelectedIcon size={22} />
                            </span>
                            <span css={css({ display: "grid", gap: 3 })}>
                                <strong css={css({ fontSize: 14 })}>{name}</strong>
                                <span
                                    css={css({ color: "var(--lumo-text-secondary)", fontSize: 10 })}
                                >
                                    Zona de {place?.radius ?? 50} m
                                </span>
                            </span>
                        </div>

                        <fieldset
                            css={css({
                                display: "grid",
                                gap: 10,
                                margin: 0,
                                padding: 0,
                                border: 0,
                            })}
                        >
                            <legend css={css({ marginBottom: 10, fontSize: 12, fontWeight: 500 })}>
                                Color
                            </legend>
                            <div
                                css={css({
                                    display: "flex",
                                    justifyContent: "space-between",
                                    gap: 4,
                                    "@media (max-width: 300px)": { gap: 1 },
                                })}
                            >
                                {PLACE_TONES.map((tone) => {
                                    const palette = PLACE_PALETTE[tone];
                                    const selected = tone === color;
                                    return (
                                        <button
                                            type="button"
                                            key={tone}
                                            aria-label={`Color ${tone}`}
                                            aria-pressed={selected}
                                            onClick={() => setColor(tone)}
                                            css={css({
                                                width: 44,
                                                height: 44,
                                                display: "grid",
                                                placeItems: "center",
                                                padding: 5,
                                                border: selected
                                                    ? `2px solid ${palette.foreground}`
                                                    : "2px solid transparent",
                                                borderRadius: 999,
                                                background: "transparent",
                                                cursor: "pointer",
                                                "@media (max-width: 300px)": {
                                                    width: 40,
                                                    height: 40,
                                                },
                                            })}
                                        >
                                            <span
                                                css={css({
                                                    width: 24,
                                                    height: 24,
                                                    borderRadius: 999,
                                                    background: palette.background,
                                                    boxShadow: `inset 0 0 0 1px ${palette.foreground}24`,
                                                })}
                                            />
                                        </button>
                                    );
                                })}
                            </div>
                        </fieldset>

                        <fieldset
                            css={css({
                                display: "grid",
                                gap: 10,
                                margin: 0,
                                padding: 0,
                                border: 0,
                            })}
                        >
                            <legend css={css({ marginBottom: 10, fontSize: 12, fontWeight: 500 })}>
                                Icono
                            </legend>
                            <div
                                css={css({
                                    display: "grid",
                                    gridTemplateColumns: "repeat(5, 1fr)",
                                    gap: 8,
                                    "@media (max-width: 340px)": {
                                        gridTemplateColumns: "repeat(4, 1fr)",
                                    },
                                    "@media (max-width: 300px)": { gap: 4 },
                                })}
                            >
                                {PLACE_ICON_OPTIONS.map((option) => {
                                    const selected = option.key === icon;
                                    return (
                                        <button
                                            type="button"
                                            key={option.key}
                                            title={option.label}
                                            aria-label={option.label}
                                            aria-pressed={selected}
                                            onClick={() => setIcon(option.key)}
                                            css={css({
                                                aspectRatio: "1",
                                                minWidth: 0,
                                                display: "grid",
                                                placeItems: "center",
                                                border: selected
                                                    ? `2px solid ${selectedPalette.foreground}`
                                                    : "1px solid var(--lumo-border)",
                                                borderRadius: 15,
                                                color: selected
                                                    ? selectedPalette.foreground
                                                    : "var(--lumo-text-secondary)",
                                                background: selected
                                                    ? selectedPalette.background
                                                    : "#fff",
                                                cursor: "pointer",
                                                transition:
                                                    "color .18s ease, background .18s ease, border-color .18s ease, transform .18s ease",
                                                "&:active": { transform: "scale(.96)" },
                                            })}
                                        >
                                            <option.icon size={20} />
                                        </button>
                                    );
                                })}
                            </div>
                        </fieldset>
                    </>
                )}
                {error && (
                    <p role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                        {error}
                    </p>
                )}
                <div
                    css={css({
                        display: "grid",
                        gridTemplateColumns:
                            step === 0 ? "1fr" : "minmax(88px, 104px) minmax(0, 1fr)",
                        gap: 10,
                    })}
                >
                    {step === 1 && (
                        <Button
                            type="button"
                            variant="secondary"
                            icon={FiChevronLeft}
                            onClick={() => setStep(0)}
                        >
                            Atrás
                        </Button>
                    )}
                    <Button
                        type="submit"
                        fullWidth
                        icon={step === 1 ? FiCheck : undefined}
                        loading={saving}
                    >
                        {step === 0 ? "Continuar" : editing ? "Guardar cambios" : "Crear lugar"}
                    </Button>
                </div>
                {place && (
                    <Button
                        type="button"
                        variant="ghost"
                        fullWidth
                        icon={FiTrash2}
                        onClick={() => onDelete(place)}
                        css={css({
                            minHeight: 46,
                            color: "var(--lumo-danger)",
                            background: "var(--lumo-danger-soft)",
                            "&:hover:not(:disabled)": {
                                color: "#9f3849",
                                background: "#f5dce1",
                            },
                        })}
                    >
                        Eliminar lugar
                    </Button>
                )}
            </form>
        </Modal>
    );
}

export function Controller() {
    const { state, dispatch, backend } = useLumo();
    const [activeTab, setActiveTab] = useState<ControllerTab>("home");
    const [locating, setLocating] = useState(false);
    const [notificationsOpen, setNotificationsOpen] = useState(false);
    const [placeOpen, setPlaceOpen] = useState(false);
    const [selectedPlace, setSelectedPlace] = useState<Place | null>(null);
    const [protectedAction, setProtectedAction] = useState<ControllerProtectedAction>(null);
    const [securityAction, setSecurityAction] = useState<GroupSecurityAction | null>(null);
    const [toast, setToast] = useState<ToastState | null>(null);
    const unread = useMemo(
        () => state.events.filter((event) => !event.read).length,
        [state.events],
    );

    const locate = async () => {
        if (locating) return;
        setLocating(true);
        try {
            const accepted = await backend.requestLocation();
            if (accepted) {
                setLocating(false);
                setToast({
                    title: "Solicitud enviada",
                    detail: "El otro teléfono actualizará su ubicación en cuanto responda",
                });
                return;
            }
        } catch (requestError) {
            setLocating(false);
            setToast({
                title: "No se ha podido localizar",
                detail: requestError instanceof Error ? requestError.message : "Inténtalo de nuevo",
            });
            return;
        }
        window.setTimeout(
            () => {
                dispatch({ type: "FINISH_LOCATE" });
                setLocating(false);
                setToast({
                    title: "Ubicación actualizada",
                    detail: `${state.demo.placeName} · precisión aproximada de 12 m`,
                });
            },
            Math.max(700, state.demo.delaySeconds * 350),
        );
    };

    const callTrackedPerson = async () => {
        try {
            const opened = await backend.openPhoneDialer(state.group.trackedPersonPhone);
            setToast(
                opened
                    ? {
                          title: "Llamada preparada",
                          detail: `Confirma la llamada a ${state.group.trackedPersonName}`,
                      }
                    : {
                          title: "Llamadas disponibles en Android",
                          detail: "La APK abrirá el marcador del teléfono",
                      },
            );
        } catch (requestError) {
            setToast({
                title: "No se ha podido preparar la llamada",
                detail:
                    requestError instanceof Error
                        ? requestError.message
                        : "Revisa el número configurado",
            });
        }
    };

    const openNotifications = async () => {
        setNotificationsOpen(true);
        try {
            const snapshot = await backend.markEventsRead();
            dispatch(
                snapshot
                    ? { type: "HYDRATE_BACKEND", payload: snapshot }
                    : { type: "MARK_EVENTS_READ" },
            );
        } catch {
            // Opening the panel remains useful with the last synchronized events.
        }
    };

    const changeTab = (tab: ControllerTab) => {
        setActiveTab(tab);
        window.requestAnimationFrame(() => window.scrollTo({ top: 0, behavior: "auto" }));
    };

    const requestPlaceDeletion = (place: Place) => {
        setPlaceOpen(false);
        window.setTimeout(() => setProtectedAction({ kind: "delete-place", place }), 220);
    };

    const deletingPlace = protectedAction?.kind === "delete-place" ? protectedAction.place : null;

    return (
        <main
            css={css({
                minHeight: "var(--lumo-viewport-height)",
                display: "flex",
                flexDirection: "column",
                background: "var(--lumo-bg)",
            })}
        >
            <header
                css={css({
                    position: "sticky",
                    zIndex: 15,
                    top: 0,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 12,
                    padding: "max(14px, var(--lumo-safe-top)) 18px 12px",
                    background: "rgba(248,245,239,.9)",
                    backdropFilter: "blur(14px)",
                })}
            >
                <div css={css({ display: "flex", alignItems: "center", gap: 10 })}>
                    <BrandMark size="small" />
                    <div css={css({ display: "grid", gap: 2 })}>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}>
                            {greeting()}
                        </span>
                        <strong css={css({ fontSize: 16, letterSpacing: "-.02em" })}>
                            {state.group.userName}
                        </strong>
                    </div>
                </div>
                <IconButton
                    label="Abrir avisos"
                    icon={FiBell}
                    badge={unread}
                    onClick={openNotifications}
                />
            </header>

            <div
                css={css({
                    width: "100%",
                    flex: 1,
                    padding: "8px 16px 28px",
                })}
            >
                {activeTab === "home" && (
                    <HomeView
                        locating={locating}
                        onLocate={locate}
                        onCall={() => void callTrackedPerson()}
                        onShowActivity={() => changeTab("activity")}
                    />
                )}
                {activeTab === "activity" && <ActivityView />}
                {activeTab === "places" && (
                    <PlacesView
                        onAdd={() => {
                            setSelectedPlace(null);
                            setPlaceOpen(true);
                        }}
                        onEdit={(place) => {
                            setSelectedPlace(place);
                            setPlaceOpen(true);
                        }}
                    />
                )}
                {activeTab === "settings" && (
                    <SettingsView
                        onToast={setToast}
                        onInvite={() => setSecurityAction("invite")}
                        onChangeMode={() => setProtectedAction({ kind: "change-mode" })}
                        onLeave={() => setSecurityAction("leave")}
                    />
                )}
            </div>

            <BottomNavigation active={activeTab} onChange={changeTab} />

            <TopSheet
                open={notificationsOpen}
                onClose={() => setNotificationsOpen(false)}
                eyebrow="Avisos locales"
                title={`Novedades de ${state.group.trackedPersonName}`}
            >
                <div css={css({ display: "grid", gap: 12 })}>
                    {state.events.length > 0 ? (
                        <Timeline events={state.events.slice(0, 5)} />
                    ) : (
                        <p css={css({ color: "var(--lumo-text-secondary)", fontSize: 13 })}>
                            No hay avisos nuevos.
                        </p>
                    )}
                    <Button
                        variant="secondary"
                        fullWidth
                        icon={FiActivity}
                        onClick={() => {
                            setNotificationsOpen(false);
                            changeTab("activity");
                        }}
                    >
                        Ver toda la actividad
                    </Button>
                </div>
            </TopSheet>

            <PlaceModal
                open={placeOpen}
                place={selectedPlace}
                onClose={() => setPlaceOpen(false)}
                onSaved={(editing) =>
                    setToast({
                        title: editing ? "Lugar actualizado" : "Lugar guardado",
                        detail: editing
                            ? "Los cambios ya aparecen en tu lista"
                            : "La zona ya aparece en tu lista",
                    })
                }
                onDelete={requestPlaceDeletion}
            />

            <ProtectedActionModal
                open={Boolean(protectedAction)}
                onClose={() => setProtectedAction(null)}
                title={deletingPlace ? `Eliminar ${deletingPlace.name}` : "Elegir otro modo"}
                description={
                    deletingPlace
                        ? "Este lugar dejará de aparecer en tus zonas habituales. Introduce el PIN para confirmar la eliminación."
                        : "Introduce el PIN para cambiar la experiencia de este teléfono."
                }
                confirmLabel={deletingPlace ? "Eliminar lugar" : "Continuar al selector"}
                icon={deletingPlace ? FiTrash2 : FiSliders}
                variant={deletingPlace ? "danger" : "primary"}
                onConfirm={async (pin) => {
                    if (deletingPlace) {
                        const snapshot = await backend.deletePlace(deletingPlace.id, pin);
                        dispatch(
                            snapshot
                                ? { type: "HYDRATE_BACKEND", payload: snapshot }
                                : { type: "DELETE_PLACE", payload: { id: deletingPlace.id } },
                        );
                        setSelectedPlace(null);
                        setToast({
                            title: "Lugar eliminado",
                            detail: `${deletingPlace.name} ya no aparece en tus zonas habituales`,
                        });
                        return;
                    }
                    dispatch({ type: "SET_MODE", payload: null });
                }}
            />

            <GroupSecurityModal action={securityAction} onClose={() => setSecurityAction(null)} />

            {toast && (
                <Toast title={toast.title} detail={toast.detail} onClose={() => setToast(null)} />
            )}
        </main>
    );
}
