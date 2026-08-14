import { useState } from "react";
import { css, keyframes } from "@emotion/react";
import {
    FiArrowRight,
    FiChevronRight,
    FiLogOut,
    FiMapPin,
    FiShield,
    FiSliders,
} from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import {
    GroupSecurityModal,
    type GroupSecurityAction,
} from "@modules/groups/components/GroupSecurityModal.tsx";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { Pill } from "@shared/components/ui.tsx";
import type { AppMode } from "@shared/types/lumo.ts";

const appear = keyframes({
    from: { opacity: 0, transform: "translateY(12px)" },
    to: { opacity: 1, transform: "translateY(0)" },
});

interface ModeSelectionProps {
    onSelect: (mode: AppMode) => void;
}

interface ModeOption {
    mode: AppMode;
    title: string;
    description: string;
    icon: IconType;
    accent: string;
    iconBackground: string;
    badge?: string;
}

const MODES: ModeOption[] = [
    {
        mode: "controller",
        title: "Cuidar a un familiar",
        description: "Consulta su estado, lugares habituales y actividad reciente.",
        icon: FiShield,
        accent: "var(--lumo-primary)",
        iconBackground: "var(--lumo-lavender)",
        badge: "Recomendado",
    },
    {
        mode: "tracker",
        title: "Compartir mi ubicación",
        description: "Una pantalla sencilla para el teléfono de la persona acompañada.",
        icon: FiMapPin,
        accent: "var(--lumo-success)",
        iconBackground: "var(--lumo-success-soft)",
    },
    {
        mode: "debug",
        title: "Abrir modo de prueba",
        description: "Simula destinos, avisos y errores sin activar el GPS.",
        icon: FiSliders,
        accent: "var(--lumo-warning)",
        iconBackground: "var(--lumo-warning-soft)",
        badge: "Datos simulados",
    },
];

export function ModeSelection({ onSelect }: ModeSelectionProps) {
    const { state } = useLumo();
    const groupName = state.group.name || "Mi familia";
    const [securityAction, setSecurityAction] = useState<GroupSecurityAction | null>(null);
    const availableModes =
        state.group.role === "member"
            ? MODES.filter((option) => option.mode === "tracker")
            : MODES.filter((option) => option.mode !== "tracker");

    return (
        <main
            css={css({
                minHeight: "var(--lumo-viewport-height)",
                display: "flex",
                flexDirection: "column",
                padding: "max(26px, var(--lumo-safe-top)) 18px max(24px, var(--lumo-safe-bottom))",
                background:
                    "radial-gradient(circle at 90% 0, rgba(165,131,225,.2), transparent 30%), var(--lumo-bg)",
            })}
        >
            <header
                css={css({
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 16,
                    marginBottom: 34,
                })}
            >
                <div css={css({ display: "flex", alignItems: "center", gap: 10 })}>
                    <BrandMark size="small" />
                    <strong css={css({ fontSize: 19, letterSpacing: "-.03em" })}>lumo</strong>
                </div>
                <button
                    type="button"
                    onClick={() => setSecurityAction("leave")}
                    css={css({
                        minHeight: 42,
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 7,
                        padding: "0 12px",
                        border: "1px solid var(--lumo-danger)",
                        borderRadius: 13,
                        color: "#fff",
                        background: "var(--lumo-danger)",
                        cursor: "pointer",
                        fontSize: 12,
                        boxShadow: "0 8px 18px rgba(180,71,88,.16)",
                        "@media (max-width: 350px)": {
                            width: 44,
                            minWidth: 44,
                            padding: 0,
                            fontSize: 0,
                            justifyContent: "center",
                        },
                    })}
                >
                    <FiLogOut size={15} aria-hidden="true" />
                    Salir del grupo
                </button>
            </header>

            <section css={css({ display: "grid", gap: 9, marginBottom: 24 })}>
                <Pill>{groupName}</Pill>
                <h1
                    css={css({
                        maxWidth: 340,
                        color: "var(--lumo-text)",
                        fontSize: 29,
                        lineHeight: 1.12,
                        letterSpacing: "-.04em",
                    })}
                >
                    ¿Cómo vas a usar este teléfono?
                </h1>
                <p
                    css={css({
                        color: "var(--lumo-text-secondary)",
                        fontSize: 13,
                        lineHeight: 1.5,
                    })}
                >
                    Elige una vista para preparar la experiencia. Podrás cambiarla después.
                </p>
            </section>

            <div css={css({ display: "grid", gap: 11 })}>
                {availableModes.map((option, index) => (
                    <button
                        key={option.mode}
                        type="button"
                        onClick={() => onSelect(option.mode)}
                        css={css({
                            width: "100%",
                            display: "grid",
                            gridTemplateColumns: "52px 1fr auto",
                            alignItems: "center",
                            gap: 13,
                            padding: "16px 14px",
                            border: "1px solid var(--lumo-border)",
                            borderRadius: 20,
                            color: "inherit",
                            textAlign: "left",
                            background: "rgba(255,255,255,.88)",
                            boxShadow: "0 8px 24px rgba(47,38,57,.045)",
                            cursor: "pointer",
                            animation: `${appear} .4s ${index * 70}ms ease both`,
                            transition:
                                "transform .2s ease, border-color .2s ease, box-shadow .2s ease",
                            "&:hover": {
                                transform: "translateY(-2px)",
                                borderColor: option.accent,
                                boxShadow: "0 12px 28px rgba(47,38,57,.08)",
                            },
                        })}
                    >
                        <span
                            css={css({
                                width: 52,
                                height: 52,
                                display: "grid",
                                placeItems: "center",
                                borderRadius: 17,
                                color: option.accent,
                                background: option.iconBackground,
                            })}
                        >
                            <option.icon size={23} aria-hidden="true" />
                        </span>
                        <span css={css({ minWidth: 0, display: "grid", gap: 5 })}>
                            <span
                                css={css({
                                    display: "flex",
                                    alignItems: "center",
                                    flexWrap: "wrap",
                                    gap: 7,
                                })}
                            >
                                <strong css={css({ color: "var(--lumo-text)", fontSize: 15 })}>
                                    {option.title}
                                </strong>
                                {option.badge && (
                                    <span
                                        css={css({
                                            padding: "3px 7px",
                                            borderRadius: 99,
                                            color: option.accent,
                                            background: option.iconBackground,
                                            fontSize: 9,
                                        })}
                                    >
                                        {option.badge}
                                    </span>
                                )}
                            </span>
                            <span
                                css={css({
                                    color: "var(--lumo-text-secondary)",
                                    fontSize: 12,
                                    lineHeight: 1.45,
                                })}
                            >
                                {option.description}
                            </span>
                        </span>
                        <FiChevronRight
                            size={19}
                            color="var(--lumo-text-muted)"
                            aria-hidden="true"
                        />
                    </button>
                ))}
            </div>

            <aside
                css={css({
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    marginTop: "auto",
                    paddingTop: 26,
                    color: "var(--lumo-text-muted)",
                    fontSize: 11,
                    lineHeight: 1.45,
                })}
            >
                <FiArrowRight size={17} aria-hidden="true" />
                Esta elección cambia la vista utilizada en este teléfono.
            </aside>
            <GroupSecurityModal action={securityAction} onClose={() => setSecurityAction(null)} />
        </main>
    );
}
