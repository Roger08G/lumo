import { useState } from "react";
import { css } from "@emotion/react";
import {
    FiAlertTriangle,
    FiBattery,
    FiChevronLeft,
    FiClock,
    FiCloudOff,
    FiCrosshair,
    FiHome,
    FiMapPin,
    FiRefreshCw,
    FiShieldOff,
    FiShoppingBag,
    FiSliders,
} from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { Button, Modal, Pill, Toast, Toggle } from "@shared/components/ui.tsx";
import { surface as panel } from "@shared/styles/surfaces.ts";
import type { DebugScenario, DemoState } from "@shared/types/lumo.ts";
import { formatClock } from "@shared/utils/format.ts";

interface ScenarioOption {
    id: DebugScenario;
    title: string;
    detail: string;
    icon: IconType;
    tone: "purple" | "green" | "amber";
}

const SCENARIOS: ScenarioOption[] = [
    { id: "home", title: "Casa", detail: "Simular llegada", icon: FiHome, tone: "green" },
    {
        id: "supermarket",
        title: "Supermercado",
        detail: "Trayecto de 14 min",
        icon: FiShoppingBag,
        tone: "purple",
    },
    {
        id: "medical",
        title: "Centro médico",
        detail: "Simular llegada",
        icon: FiMapPin,
        tone: "purple",
    },
    {
        id: "away",
        title: "Fuera de zona",
        detail: "Simular salida",
        icon: FiCrosshair,
        tone: "purple",
    },
    {
        id: "offline",
        title: "Sin conexión",
        detail: "Aviso de 30 min",
        icon: FiCloudOff,
        tone: "amber",
    },
    {
        id: "permission",
        title: "Permiso revocado",
        detail: "Bloquear ubicación",
        icon: FiShieldOff,
        tone: "amber",
    },
    {
        id: "battery",
        title: "Batería baja",
        detail: "Cambiar al 12 %",
        icon: FiBattery,
        tone: "amber",
    },
];

const tones = {
    purple: { color: "var(--lumo-primary)", background: "var(--lumo-lavender)" },
    green: { color: "var(--lumo-success)", background: "var(--lumo-success-soft)" },
    amber: { color: "var(--lumo-warning)", background: "var(--lumo-warning-soft)" },
};

export function DebugLab() {
    const { state, dispatch, backend } = useLumo();
    const [toast, setToast] = useState<{ title: string; detail?: string } | null>(null);
    const [resetOpen, setResetOpen] = useState(false);

    const runScenario = async (scenario: ScenarioOption) => {
        try {
            const snapshot = await backend.applyDebugScenario(scenario.id);
            dispatch(
                snapshot
                    ? { type: "HYDRATE_BACKEND", payload: snapshot }
                    : { type: "APPLY_SCENARIO", payload: scenario.id },
            );
            setToast({
                title: `${scenario.title} aplicado`,
                detail: "El controlador y el historial ya reflejan este estado",
            });
        } catch (requestError) {
            setToast({
                title: "No se ha podido aplicar",
                detail: requestError instanceof Error ? requestError.message : "Inténtalo de nuevo",
            });
        }
    };

    return (
        <main
            css={css({
                minHeight: "100dvh",
                padding:
                    "max(16px, env(safe-area-inset-top)) 16px max(26px, env(safe-area-inset-bottom))",
                background:
                    "linear-gradient(180deg, rgba(251,239,223,.42), transparent 240px), var(--lumo-bg)",
            })}
        >
            <header
                css={css({
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 12,
                    marginBottom: 22,
                })}
            >
                <button
                    type="button"
                    aria-label="Volver al selector de modo"
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
                <div css={css({ display: "flex", alignItems: "center", gap: 9 })}>
                    <BrandMark size="small" />
                    <strong css={css({ fontSize: 16 })}>Laboratorio</strong>
                </div>
                <Pill tone="amber">Simulado</Pill>
            </header>

            <section css={css({ display: "grid", gap: 7, marginBottom: 18 })}>
                <Pill>
                    <FiSliders /> Herramientas de interfaz
                </Pill>
                <h1 css={css({ fontSize: 27, lineHeight: 1.12, letterSpacing: "-.04em" })}>
                    Prueba cada estado de Lumo
                </h1>
                <p
                    css={css({
                        color: "var(--lumo-text-secondary)",
                        fontSize: 12,
                        lineHeight: 1.5,
                    })}
                >
                    Los cambios son locales, persisten al recargar y nunca utilizan el GPS real.
                </p>
            </section>

            <section
                css={css(panel, {
                    display: "grid",
                    gridTemplateColumns: "auto 1fr auto",
                    alignItems: "center",
                    gap: 12,
                    marginBottom: 18,
                    padding: 14,
                })}
            >
                <span
                    css={css({
                        width: 46,
                        height: 46,
                        display: "grid",
                        placeItems: "center",
                        borderRadius: 15,
                        color:
                            state.demo.connection === "online" &&
                            state.demo.permission === "granted"
                                ? "var(--lumo-primary)"
                                : "var(--lumo-warning)",
                        background:
                            state.demo.connection === "online" &&
                            state.demo.permission === "granted"
                                ? "var(--lumo-lavender)"
                                : "var(--lumo-warning-soft)",
                    })}
                >
                    <FiMapPin size={21} />
                </span>
                <span css={css({ minWidth: 0, display: "grid", gap: 4 })}>
                    <span css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}>
                        ESTADO ACTUAL
                    </span>
                    <strong
                        css={css({
                            overflow: "hidden",
                            fontSize: 14,
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                        })}
                    >
                        {state.demo.statusText}
                    </strong>
                    <span css={css({ color: "var(--lumo-text-secondary)", fontSize: 10 })}>
                        {state.demo.battery} % ·{" "}
                        {state.demo.accuracy === "high" ? "Precisión alta" : "Precisión limitada"}
                    </span>
                </span>
                <Pill tone={state.demo.connection === "online" ? "green" : "amber"}>
                    {state.demo.connection === "online" ? "Online" : "Offline"}
                </Pill>
            </section>

            <section css={css({ display: "grid", gap: 10, marginBottom: 20 })}>
                <div
                    css={css({
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                    })}
                >
                    <h2 css={css({ fontSize: 17, letterSpacing: "-.02em" })}>Escenarios rápidos</h2>
                    <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10 })}>
                        Toca para ejecutar
                    </span>
                </div>
                <div css={css({ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 9 })}>
                    {SCENARIOS.map((scenario, index) => {
                        const tone = tones[scenario.tone];
                        return (
                            <button
                                key={scenario.id}
                                type="button"
                                onClick={() => runScenario(scenario)}
                                css={css({
                                    minHeight: index === 0 ? 104 : 96,
                                    display: "grid",
                                    alignContent: "space-between",
                                    justifyItems: "start",
                                    gap: 10,
                                    padding: 13,
                                    border: "1px solid var(--lumo-border)",
                                    borderRadius: 19,
                                    color: "inherit",
                                    textAlign: "left",
                                    background: "rgba(255,255,255,.9)",
                                    cursor: "pointer",
                                    boxShadow: "0 7px 18px rgba(47,38,57,.035)",
                                    transition: "transform .2s ease, border-color .2s ease",
                                    "&:hover": {
                                        transform: "translateY(-2px)",
                                        borderColor: tone.color,
                                    },
                                    "&:first-of-type": {
                                        gridColumn: "1 / -1",
                                        gridTemplateColumns: "46px 1fr auto",
                                        alignItems: "center",
                                        alignContent: "center",
                                    },
                                })}
                            >
                                <span
                                    css={css({
                                        width: 42,
                                        height: 42,
                                        display: "grid",
                                        placeItems: "center",
                                        borderRadius: 14,
                                        color: tone.color,
                                        background: tone.background,
                                    })}
                                >
                                    <scenario.icon size={19} />
                                </span>
                                <span css={css({ display: "grid", gap: 3 })}>
                                    <strong css={css({ fontSize: 12 })}>{scenario.title}</strong>
                                    <span
                                        css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}
                                    >
                                        {scenario.detail}
                                    </span>
                                </span>
                                {index === 0 && <Pill tone="green">Estado seguro</Pill>}
                            </button>
                        );
                    })}
                </div>
            </section>

            <section
                css={css(panel, {
                    display: "grid",
                    gap: 4,
                    marginBottom: 18,
                    padding: "10px 15px 14px",
                })}
            >
                <h2 css={css({ margin: "6px 0 5px", fontSize: 17, letterSpacing: "-.02em" })}>
                    Controles manuales
                </h2>
                <Toggle
                    label="Conexión disponible"
                    description="Alterna el estado online del tracker"
                    checked={state.demo.connection === "online"}
                    onChange={(checked) =>
                        dispatch({
                            type: "SET_CONNECTION",
                            payload: checked ? "online" : "offline",
                        })
                    }
                />
                <div css={css({ height: 1, background: "var(--lumo-border)" })} />
                <Toggle
                    label="Permiso de ubicación"
                    description="Simula el permiso concedido por Android"
                    checked={state.demo.permission === "granted"}
                    onChange={(checked) =>
                        dispatch({
                            type: "SET_PERMISSION",
                            payload: checked ? "granted" : "revoked",
                        })
                    }
                />
                <div css={css({ height: 1, background: "var(--lumo-border)" })} />
                <label css={css({ display: "grid", gap: 9, padding: "12px 0", fontSize: 12 })}>
                    <span
                        css={css({
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 12,
                        })}
                    >
                        <span css={css({ display: "inline-flex", alignItems: "center", gap: 7 })}>
                            <FiBattery /> Nivel de batería
                        </span>
                        <strong>{state.demo.battery} %</strong>
                    </span>
                    <input
                        type="range"
                        min={1}
                        max={100}
                        value={state.demo.battery}
                        onChange={(event) =>
                            dispatch({ type: "SET_BATTERY", payload: Number(event.target.value) })
                        }
                        css={css({ width: "100%", accentColor: "var(--lumo-primary)" })}
                    />
                </label>
                <div css={css({ height: 1, background: "var(--lumo-border)" })} />
                <fieldset css={css({ display: "grid", gap: 9, padding: "12px 0 6px", border: 0 })}>
                    <legend css={css({ marginBottom: 9, fontSize: 12 })}>Precisión simulada</legend>
                    <div
                        css={css({
                            display: "grid",
                            gridTemplateColumns: "repeat(3, 1fr)",
                            gap: 6,
                        })}
                    >
                        {(["high", "balanced", "low"] as DemoState["accuracy"][]).map(
                            (accuracy) => {
                                const labels = { high: "Alta", balanced: "Media", low: "Baja" };
                                const selected = state.demo.accuracy === accuracy;
                                return (
                                    <button
                                        key={accuracy}
                                        type="button"
                                        aria-pressed={selected}
                                        onClick={() =>
                                            dispatch({ type: "SET_ACCURACY", payload: accuracy })
                                        }
                                        css={css({
                                            minHeight: 42,
                                            border: `1px solid ${selected ? "var(--lumo-primary)" : "var(--lumo-border)"}`,
                                            borderRadius: 12,
                                            color: selected
                                                ? "var(--lumo-primary)"
                                                : "var(--lumo-text-secondary)",
                                            background: selected ? "var(--lumo-lavender)" : "#fff",
                                            cursor: "pointer",
                                            fontSize: 11,
                                        })}
                                    >
                                        {labels[accuracy]}
                                    </button>
                                );
                            },
                        )}
                    </div>
                </fieldset>
                <label
                    css={css({
                        display: "grid",
                        gap: 7,
                        paddingTop: 9,
                        color: "var(--lumo-text)",
                        fontSize: 12,
                    })}
                >
                    <span css={css({ display: "inline-flex", alignItems: "center", gap: 7 })}>
                        <FiClock /> Retraso de respuesta
                    </span>
                    <select
                        value={state.demo.delaySeconds}
                        onChange={(event) =>
                            dispatch({ type: "SET_DELAY", payload: Number(event.target.value) })
                        }
                        css={css({
                            minHeight: 46,
                            padding: "0 12px",
                            border: "1px solid var(--lumo-border)",
                            borderRadius: 13,
                            color: "var(--lumo-text)",
                            background: "#fff",
                        })}
                    >
                        <option value={0}>Sin retraso</option>
                        <option value={2}>2 segundos</option>
                        <option value={5}>5 segundos</option>
                        <option value={10}>10 segundos</option>
                    </select>
                </label>
            </section>

            <section css={css(panel, { marginBottom: 18, padding: "15px" })}>
                <div
                    css={css({
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                        marginBottom: 8,
                    })}
                >
                    <h2 css={css({ fontSize: 17 })}>Registro de eventos</h2>
                    <Pill tone="neutral">{state.events.length}</Pill>
                </div>
                <div css={css({ display: "grid" })}>
                    {state.events.length === 0 && (
                        <p
                            css={css({
                                padding: "18px 0",
                                color: "var(--lumo-text-secondary)",
                                fontSize: 11,
                                textAlign: "center",
                            })}
                        >
                            Aún no hay eventos en las últimas 24 horas.
                        </p>
                    )}
                    {state.events.slice(0, 5).map((event, index) => (
                        <div
                            key={event.id}
                            css={css({
                                display: "grid",
                                gridTemplateColumns: "8px 1fr auto",
                                alignItems: "center",
                                gap: 9,
                                padding: "10px 0",
                                borderBottom:
                                    index === Math.min(state.events.length, 5) - 1
                                        ? 0
                                        : "1px solid var(--lumo-border)",
                            })}
                        >
                            <span
                                css={css({
                                    width: 7,
                                    height: 7,
                                    borderRadius: "50%",
                                    background:
                                        event.kind === "warning"
                                            ? "var(--lumo-warning)"
                                            : "var(--lumo-primary)",
                                })}
                            />
                            <span css={css({ minWidth: 0, display: "grid", gap: 3 })}>
                                <strong
                                    css={css({
                                        overflow: "hidden",
                                        fontSize: 11,
                                        textOverflow: "ellipsis",
                                        whiteSpace: "nowrap",
                                    })}
                                >
                                    {event.title}
                                </strong>
                                <span css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}>
                                    {event.detail}
                                </span>
                            </span>
                            <time css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}>
                                {formatClock(event.at)}
                            </time>
                        </div>
                    ))}
                </div>
            </section>

            <div css={css({ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 9 })}>
                <Button
                    variant="secondary"
                    fullWidth
                    icon={FiRefreshCw}
                    onClick={() => setResetOpen(true)}
                >
                    Restablecer
                </Button>
                <Button
                    fullWidth
                    icon={FiCrosshair}
                    onClick={() => dispatch({ type: "SET_MODE", payload: "controller" })}
                >
                    Ver controlador
                </Button>
            </div>

            <Modal
                open={resetOpen}
                onClose={() => setResetOpen(false)}
                eyebrow="Datos simulados"
                title="¿Restablecer la demostración?"
            >
                <div css={css({ display: "grid", gap: 15 })}>
                    <div
                        css={css({
                            display: "flex",
                            alignItems: "flex-start",
                            gap: 10,
                            padding: 13,
                            borderRadius: 15,
                            color: "var(--lumo-warning)",
                            background: "var(--lumo-warning-soft)",
                            fontSize: 12,
                            lineHeight: 1.5,
                        })}
                    >
                        <FiAlertTriangle size={18} css={css({ flex: "0 0 auto" })} />
                        Se recuperarán los lugares, eventos y estado inicial. El grupo seguirá
                        vinculado.
                    </div>
                    <Button
                        fullWidth
                        variant="danger"
                        icon={FiRefreshCw}
                        onClick={() => {
                            dispatch({ type: "RESET_DEMO" });
                            setResetOpen(false);
                            setToast({
                                title: "Demo restablecida",
                                detail: "Se ha recuperado el estado inicial",
                            });
                        }}
                    >
                        Sí, restablecer
                    </Button>
                </div>
            </Modal>

            {toast && (
                <Toast title={toast.title} detail={toast.detail} onClose={() => setToast(null)} />
            )}
        </main>
    );
}
