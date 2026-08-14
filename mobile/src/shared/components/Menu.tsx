import React from "react";
import { css } from "@emotion/react";
import type { IconType } from "react-icons";

import { IoAddOutline } from "react-icons/io5";
import { FiUserPlus } from "react-icons/fi";
import { LuBug } from "react-icons/lu";

type NavigationMap = { title: string; Icon: IconType };

const NAVIGATION_MAP: NavigationMap[] = [
    { title: "qr", Icon: FiUserPlus },
    { title: "add", Icon: IoAddOutline },
    { title: "debug", Icon: LuBug },
];

const NavigationStyles = css({
    display: "flex",
    flexDirection: "row",
    justifyContent: "space-between",
    padding: "0 3.5rem",
    alignItems: "center",
    height: "var(--lumo-viewport-height)",
    margin: "auto 0",
});

const LiElementStyles = (main: boolean) =>
    css({
        listStyle: "none",
        fontSize: main ? "1.8rem" : "1.5rem",
        transform: main ? "translateY(-45px)" : "none",
    });

export const Menu: React.FC = () => {
    return (
        <nav css={NavigationStyles}>
            {NAVIGATION_MAP.map((nav, index) => (
                <li
                    key={index}
                    title={String(nav.title)}
                    css={LiElementStyles(nav.title === "add" ? true : false)}
                >
                    <i>
                        <nav.Icon />
                    </i>
                </li>
            ))}
        </nav>
    );
};
