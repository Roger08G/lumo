import { css } from "@emotion/react";

interface GroupButtonsProps {
    onCreate: () => void;
    onJoin: () => void;
}

const buttonBase = css({
    width: "100%",
    minHeight: 56,
    borderRadius: 999,
    fontSize: 15,
    fontWeight: 500,
    cursor: "pointer",
    transition: "transform .2s ease, box-shadow .2s ease, background .2s ease",
    "&:active": { transform: "scale(.985)" },
});

export function GroupButtons({ onCreate, onJoin }: GroupButtonsProps) {
    return (
        <div css={css({ width: "100%", display: "grid", gap: 12 })}>
            <button
                type="button"
                onClick={onCreate}
                css={css(buttonBase, {
                    border: 0,
                    color: "#fff",
                    background: "var(--lumo-primary)",
                    boxShadow: "0 10px 22px rgba(104,66,166,.18)",
                    "&:hover": { background: "var(--lumo-primary-dark)" },
                })}
            >
                Crear un grupo
            </button>
            <button
                type="button"
                onClick={onJoin}
                css={css(buttonBase, {
                    border: "1px solid var(--lumo-border-strong)",
                    color: "var(--lumo-primary)",
                    background: "#fff",
                    "&:hover": { background: "#fdfbff" },
                })}
            >
                Unirme a un grupo
            </button>
        </div>
    );
}
