import { useState } from "react";
import { css } from "@emotion/react";
import {
    FiBatteryCharging,
    FiCheck,
    FiChevronLeft,
    FiInfo,
    FiMapPin,
    FiShield,
} from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { Button, Pill } from "@shared/components/ui.tsx";
import type { PreferencesState } from "@shared/types/lumo.ts";

interface PermissionItem {
    key: keyof PreferencesState["trackerConsents"];
    title: string;
    detail: string;
    icon: IconType;
}

const PERMISSIONS: PermissionItem[] = [
    {
        key: "preciseLocation",
        title: "Ubicación precisa",
        detail: "Permite reconocer llegadas a los lugares habituales.",
        icon: FiMapPin,
    },
    {
        key: "backgroundLocation",
        title: "Acceso en segundo plano",
        detail: "Mantiene la protección cuando Lumo no está abierto.",
        icon: FiShield,
    },
    {
        key: "batteryProtection",
        title: "Uso de batería estable",
        detail: "Evita que el sistema detenga la protección por ahorro de energía.",
        icon: FiBatteryCharging,
    },
];

export function TrackerSetup() {
    const { state, dispatch, backend } = useLumo();
    const [activating, setActivating] = useState(false);
    const [error, setError] = useState("");
    const consents = state.preferences.trackerConsents;
    const ready = Object.values(consents).every(Boolean);

    return (
        <main
            css={css({
                minHeight: "100dvh",
                display: "flex",
                flexDirection: "column",
                padding:
                    "max(22px, env(safe-area-inset-top)) 18px max(22px, env(safe-area-inset-bottom))",
                background: "var(--lumo-bg)",
            })}
        >
            <header
                css={css({
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    marginBottom: 28,
                })}
            >
                {state.group.role === "supervisor" ? (
                    <button
                        type="button"
                        aria-label="Volver a elegir modo"
                        onClick={() => dispatch({ type: "SET_MODE", payload: null })}
                        css={css({
                            width: 44,
                            height: 44,
                            display: "grid",
                            placeItems: "center",
                            border: "1px solid var(--lumo-border)",
                            borderRadius: 14,
                            color: "var(--lumo-text)",
                            background: "#fff",
                            cursor: "pointer",
                        })}
                    >
                        <FiChevronLeft size={20} />
                    </button>
                ) : (
                    <span aria-hidden="true" css={css({ width: 44, height: 44 })} />
                )}
                <BrandMark size="small" />
                <Pill tone="neutral">
                    {state.group.role === "member" ? "Persona acompañada" : "Vista previa"}
                </Pill>
            </header>

            <section css={css({ display: "grid", gap: 9, marginBottom: 22 })}>
                <Pill tone="green">Configuración guiada</Pill>
                <h1
                    css={css({
                        maxWidth: 340,
                        fontSize: 28,
                        lineHeight: 1.12,
                        letterSpacing: "-.04em",
                    })}
                >
                    Prepara la protección familiar
                </h1>
                <p
                    css={css({
                        color: "var(--lumo-text-secondary)",
                        fontSize: 13,
                        lineHeight: 1.55,
                    })}
                >
                    Lumo debe permanecer visible y contar con el consentimiento de la persona que
                    comparte su ubicación.
                </p>
            </section>

            <div css={css({ display: "grid", gap: 10 })}>
                {PERMISSIONS.map((permission) => {
                    const active = consents[permission.key];
                    return (
                        <article
                            key={permission.key}
                            css={css({
                                display: "grid",
                                gridTemplateColumns: "46px minmax(0, 1fr) auto",
                                alignItems: "center",
                                gap: 12,
                                padding: 14,
                                border: `1px solid ${active ? "#bcdcca" : "var(--lumo-border)"}`,
                                borderRadius: 19,
                                background: active ? "var(--lumo-success-soft)" : "#fff",
                                transition: "border-color .2s ease, background .2s ease",
                            })}
                        >
                            <span
                                css={css({
                                    width: 46,
                                    height: 46,
                                    display: "grid",
                                    placeItems: "center",
                                    borderRadius: 15,
                                    color: active ? "var(--lumo-success)" : "var(--lumo-primary)",
                                    background: active ? "#fff" : "var(--lumo-lavender)",
                                })}
                            >
                                <permission.icon size={21} aria-hidden="true" />
                            </span>
                            <span css={css({ display: "grid", gap: 4 })}>
                                <strong css={css({ fontSize: 14 })}>{permission.title}</strong>
                                <span
                                    css={css({
                                        color: "var(--lumo-text-secondary)",
                                        fontSize: 11,
                                        lineHeight: 1.4,
                                    })}
                                >
                                    {permission.detail}
                                </span>
                            </span>
                            <button
                                type="button"
                                aria-label={`${active ? "Desactivar" : "Activar"} ${permission.title}`}
                                onClick={() =>
                                    dispatch({
                                        type: "SET_TRACKER_CONSENT",
                                        payload: { key: permission.key, value: !active },
                                    })
                                }
                                css={css({
                                    minWidth: 45,
                                    minHeight: 44,
                                    display: "grid",
                                    placeItems: "center",
                                    padding: "0 10px",
                                    border: 0,
                                    borderRadius: 13,
                                    color: active ? "#fff" : "var(--lumo-primary)",
                                    background: active
                                        ? "var(--lumo-success)"
                                        : "var(--lumo-lavender)",
                                    cursor: "pointer",
                                    fontSize: 11,
                                    "@media (max-width: 350px)": {
                                        minWidth: 44,
                                        padding: 0,
                                        fontSize: active ? 11 : 0,
                                        "&::after": active
                                            ? undefined
                                            : { content: '"+"', fontSize: 18 },
                                    },
                                })}
                            >
                                {active ? <FiCheck size={18} /> : "Activar"}
                            </button>
                        </article>
                    );
                })}
            </div>

            <aside
                css={css({
                    display: "flex",
                    alignItems: "flex-start",
                    gap: 9,
                    margin: "18px 0",
                    padding: 13,
                    borderRadius: 15,
                    color: "var(--lumo-text-secondary)",
                    background: "#efedf0",
                    fontSize: 11,
                    lineHeight: 1.5,
                })}
            >
                <FiInfo size={16} css={css({ flex: "0 0 auto", marginTop: 1 })} />
                Android puede mostrar sus propios avisos para confirmar estos permisos.
            </aside>

            {error && (
                <p role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                    {error}
                </p>
            )}

            <Button
                fullWidth
                icon={FiShield}
                disabled={!ready}
                loading={activating}
                onClick={async () => {
                    setActivating(true);
                    setError("");
                    try {
                        const snapshot = await backend.completeTracking();
                        dispatch(
                            snapshot
                                ? { type: "HYDRATE_BACKEND", payload: snapshot }
                                : { type: "COMPLETE_TRACKER_SETUP" },
                        );
                    } catch (requestError) {
                        setError(
                            requestError instanceof Error
                                ? requestError.message
                                : "No se ha podido activar el seguimiento",
                        );
                        setActivating(false);
                    }
                }}
                css={css({ marginTop: "auto" })}
            >
                {ready ? "Activar protección" : "Completa los tres pasos"}
            </Button>
        </main>
    );
}
