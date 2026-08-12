import React from "react";
import { css } from "@emotion/react";
import type { IconType } from "react-icons";

import { FiUser } from "react-icons/fi";
import { FiUsers } from "react-icons/fi";

type ButtonsMap = { title: string; styleComplete?: boolean; Icon?: IconType };

const BUTTONS_MAP: ButtonsMap[] = [
    { title: "Crear un Grupo", styleComplete: true, Icon: FiUser },
    { title: "Unirse a un Grupo", styleComplete: false, Icon: FiUsers },
];

const ButtonStyles = (styleComplete?: boolean) =>
    css({
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        flexDirection: "row",
        gap: ".5rem",
        width: "100%",
        padding: ".65rem 2.5rem",
        borderRadius: "999px",
        fontSize: ".95rem",
        backgroundColor: styleComplete ? "var(--lumo-purple)" : "none",
        border: styleComplete ? "none" : "1px solid var(--lumo-purple)",
        color: styleComplete ? "var(--lumo-cream)" : "var(--lumo-purple)",
        i: {
            color: styleComplete ? "var(--lumo-cream)" : "var(--lumo-purple)",
            fontSize: "1.15rem",
        },
    });

export const AuthButtons: React.FC = () => {
    return (
        <div style={{ display: "flex", flexDirection: "column", gap: ".65rem" }}>
            {BUTTONS_MAP.map((button, index) => (
                <button key={index} css={ButtonStyles(button.styleComplete)}>
                    {button.Icon && (
                        <i>
                            <button.Icon />
                        </i>
                    )}
                    {String(button.title)}
                </button>
            ))}
        </div>
    );
};
