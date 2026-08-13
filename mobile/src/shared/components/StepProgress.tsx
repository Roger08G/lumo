import { css } from "@emotion/react";

interface StepProgressProps {
    current: number;
    total: number;
    variant?: "dots" | "bars";
}

export function StepProgress({ current, total, variant = "dots" }: StepProgressProps) {
    return (
        <div
            role="progressbar"
            aria-label={`Paso ${current + 1} de ${total}`}
            aria-valuemin={1}
            aria-valuemax={total}
            aria-valuenow={current + 1}
            css={css({
                display: "grid",
                gridTemplateColumns:
                    variant === "bars" ? `repeat(${total}, 1fr)` : `repeat(${total}, auto)`,
                justifyContent: variant === "dots" ? "start" : "stretch",
                gap: variant === "dots" ? 6 : 7,
            })}
        >
            {Array.from({ length: total }, (_, index) => {
                const active = variant === "bars" ? index <= current : index === current;
                return (
                    <span
                        key={index}
                        css={css({
                            width: variant === "dots" ? (active ? 22 : 6) : "auto",
                            height: variant === "dots" ? 6 : 5,
                            borderRadius: 999,
                            background: active
                                ? "var(--lumo-primary)"
                                : "var(--lumo-border-strong)",
                            transition: "width .24s ease, background .24s ease",
                        })}
                    />
                );
            })}
        </div>
    );
}
