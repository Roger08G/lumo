import { css, keyframes } from "@emotion/react";

import lumoLogo from "@tauri/icons/icon.png";

const reveal = keyframes({
    from: { opacity: 0, transform: "translateY(8px) scale(.94)" },
    to: { opacity: 1, transform: "translateY(0) scale(1)" },
});

interface BrandMarkProps {
    size?: "small" | "medium" | "large";
    animated?: boolean;
}

const sizes = {
    small: 38,
    medium: 54,
    large: 86,
};

export function BrandMark({ size = "medium", animated = false }: BrandMarkProps) {
    const dimension = sizes[size];

    return (
        <img
            src={lumoLogo}
            alt=""
            aria-hidden="true"
            width={dimension}
            height={dimension}
            css={css({
                display: "block",
                width: dimension,
                height: dimension,
                flex: "0 0 auto",
                borderRadius: Math.round(dimension * 0.23),
                objectFit: "cover",
                boxShadow:
                    size === "large"
                        ? "0 18px 36px rgba(75,47,117,.2)"
                        : "0 8px 18px rgba(75,47,117,.14)",
                animation: animated ? `${reveal} .55s cubic-bezier(.22,.9,.35,1) both` : undefined,
            })}
        />
    );
}
