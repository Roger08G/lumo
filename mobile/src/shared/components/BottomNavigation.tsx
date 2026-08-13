import { css } from "@emotion/react";
import { FiActivity, FiHome, FiMapPin, FiSettings } from "react-icons/fi";
import type { IconType } from "react-icons";

export type ControllerTab = "home" | "activity" | "places" | "settings";

interface NavigationItem {
    id: ControllerTab;
    label: string;
    icon: IconType;
}

const ITEMS: NavigationItem[] = [
    { id: "home", label: "Inicio", icon: FiHome },
    { id: "activity", label: "Actividad", icon: FiActivity },
    { id: "places", label: "Lugares", icon: FiMapPin },
    { id: "settings", label: "Ajustes", icon: FiSettings },
];

interface BottomNavigationProps {
    active: ControllerTab;
    onChange: (tab: ControllerTab) => void;
}

export function BottomNavigation({ active, onChange }: BottomNavigationProps) {
    return (
        <nav
            aria-label="Navegación principal"
            css={css({
                position: "sticky",
                zIndex: 20,
                bottom: 0,
                display: "grid",
                gridTemplateColumns: "repeat(4, 1fr)",
                padding: "7px 8px max(7px, env(safe-area-inset-bottom))",
                borderTop: "1px solid var(--lumo-border)",
                background: "rgba(255,255,255,.92)",
                backdropFilter: "blur(16px)",
                boxShadow: "0 -10px 32px rgba(47,38,57,.05)",
            })}
        >
            {ITEMS.map((item) => {
                const selected = active === item.id;
                return (
                    <button
                        key={item.id}
                        type="button"
                        aria-current={selected ? "page" : undefined}
                        aria-label={item.label}
                        onClick={() => onChange(item.id)}
                        css={css({
                            minHeight: 58,
                            display: "grid",
                            placeItems: "center",
                            alignContent: "center",
                            gap: 4,
                            border: 0,
                            borderRadius: 15,
                            color: selected ? "var(--lumo-primary)" : "var(--lumo-text-muted)",
                            background: selected ? "var(--lumo-lavender)" : "transparent",
                            cursor: "pointer",
                            fontSize: 10,
                            transition: "color .2s ease, background .2s ease",
                        })}
                    >
                        <item.icon size={20} aria-hidden="true" />
                        <span>{item.label}</span>
                    </button>
                );
            })}
        </nav>
    );
}
