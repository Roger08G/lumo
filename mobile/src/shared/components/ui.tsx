import {
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
    type ButtonHTMLAttributes,
    type InputHTMLAttributes,
    type ReactNode,
} from "react";
import { css, keyframes, type CSSObject } from "@emotion/react";
import type { IconType } from "react-icons";
import { FiX } from "react-icons/fi";
import gsap from "gsap";

const spin = keyframes({
    to: { transform: "rotate(360deg)" },
});

const toastIn = keyframes({
    from: { opacity: 0, transform: "translate(-50%, 14px) scale(0.97)" },
    to: { opacity: 1, transform: "translate(-50%, 0) scale(1)" },
});

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ButtonVariant;
    icon?: IconType;
    fullWidth?: boolean;
    loading?: boolean;
}

const buttonVariants: Record<ButtonVariant, CSSObject> = {
    primary: {
        color: "#fff",
        background: "linear-gradient(135deg, var(--lumo-primary), #7c54c5)",
        borderColor: "transparent",
        boxShadow: "0 10px 24px rgba(104, 66, 166, 0.22)",
        "&:hover:not(:disabled)": {
            transform: "translateY(-1px)",
            boxShadow: "0 13px 28px rgba(104,66,166,.27)",
        },
    },
    secondary: {
        color: "var(--lumo-primary)",
        background: "#fff",
        borderColor: "var(--lumo-border-strong)",
        boxShadow: "0 4px 14px rgba(47, 38, 57, 0.04)",
        "&:hover:not(:disabled)": { borderColor: "var(--lumo-accent)", background: "#fdfbff" },
    },
    ghost: {
        color: "var(--lumo-text-secondary)",
        background: "transparent",
        borderColor: "transparent",
        "&:hover:not(:disabled)": {
            color: "var(--lumo-primary)",
            background: "var(--lumo-lavender)",
        },
    },
    danger: {
        color: "#fff",
        background: "var(--lumo-danger)",
        borderColor: "transparent",
        boxShadow: "0 9px 20px rgba(180,71,88,.16)",
        "&:hover:not(:disabled)": { background: "#9f3849" },
    },
};

export function Button({
    variant = "primary",
    icon: Icon,
    fullWidth = false,
    loading = false,
    children,
    disabled,
    ...props
}: ButtonProps) {
    return (
        <button
            type="button"
            disabled={disabled || loading}
            css={css(
                {
                    minHeight: 52,
                    width: fullWidth ? "100%" : "auto",
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    gap: 9,
                    padding: "0 20px",
                    border: "1px solid",
                    borderRadius: 15,
                    fontSize: 15,
                    fontWeight: 500,
                    cursor: "pointer",
                    transition:
                        "transform .2s ease, box-shadow .2s ease, background .2s ease, border-color .2s ease",
                    "&:active:not(:disabled)": { transform: "translateY(1px)" },
                    "&:disabled": { opacity: 0.55, cursor: "not-allowed", boxShadow: "none" },
                    "@media (max-width: 340px)": { padding: "0 14px", fontSize: 14 },
                },
                buttonVariants[variant],
            )}
            {...props}
        >
            {loading ? (
                <span
                    aria-hidden="true"
                    css={css({
                        width: 17,
                        height: 17,
                        border: "2px solid currentColor",
                        borderRightColor: "transparent",
                        borderRadius: "50%",
                        animation: `${spin} .7s linear infinite`,
                    })}
                />
            ) : (
                Icon && <Icon aria-hidden="true" size={18} />
            )}
            {children}
        </button>
    );
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    label: string;
    icon: IconType;
    badge?: number;
}

export function IconButton({ label, icon: Icon, badge, ...props }: IconButtonProps) {
    return (
        <button
            type="button"
            aria-label={label}
            title={label}
            css={css({
                position: "relative",
                width: 46,
                height: 46,
                display: "inline-grid",
                placeItems: "center",
                flex: "0 0 auto",
                border: "1px solid var(--lumo-border)",
                borderRadius: 15,
                color: "var(--lumo-text)",
                background: "rgba(255,255,255,.82)",
                cursor: "pointer",
                transition: "background .2s ease, transform .2s ease",
                "&:hover": { background: "#fff", transform: "translateY(-1px)" },
            })}
            {...props}
        >
            <Icon size={20} aria-hidden="true" />
            {Boolean(badge) && (
                <span
                    css={css({
                        position: "absolute",
                        top: -4,
                        right: -4,
                        minWidth: 19,
                        height: 19,
                        display: "grid",
                        placeItems: "center",
                        padding: "0 5px",
                        border: "2px solid var(--lumo-bg)",
                        borderRadius: 10,
                        color: "#fff",
                        background: "var(--lumo-danger)",
                        fontSize: 10,
                        lineHeight: 1,
                    })}
                >
                    {Math.min(badge ?? 0, 9)}
                </span>
            )}
        </button>
    );
}

interface FieldProps extends InputHTMLAttributes<HTMLInputElement> {
    label: string;
    icon?: IconType;
    trailing?: ReactNode;
    error?: string;
}

export function Field({ label, icon: Icon, trailing, error, id, ...props }: FieldProps) {
    return (
        <label
            htmlFor={id}
            css={css({ display: "grid", gap: 7, color: "var(--lumo-text)", fontSize: 13 })}
        >
            <span>{label}</span>
            <span
                css={css({
                    minHeight: 52,
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "0 14px",
                    border: `1px solid ${error ? "var(--lumo-danger)" : "var(--lumo-border)"}`,
                    borderRadius: 15,
                    background: "rgba(255,255,255,.86)",
                    transition: "border-color .2s ease, box-shadow .2s ease, background .2s ease",
                    "&:focus-within": {
                        borderColor: error ? "var(--lumo-danger)" : "var(--lumo-primary)",
                        boxShadow: error
                            ? "0 0 0 3px rgba(180,71,88,.1)"
                            : "0 0 0 3px rgba(104,66,166,.1)",
                        background: "#fff",
                    },
                })}
            >
                {Icon && <Icon size={18} color="var(--lumo-text-muted)" aria-hidden="true" />}
                <input
                    id={id}
                    css={css({
                        width: "100%",
                        minWidth: 0,
                        border: 0,
                        outline: 0,
                        color: "var(--lumo-text)",
                        background: "transparent",
                        fontSize: 16,
                        appearance: "none",
                        boxShadow: "none",
                        "&:focus": { outline: "none" },
                        "&:focus-visible": { outline: "none" },
                        "&::placeholder": { color: "#9b94a1" },
                    })}
                    aria-invalid={Boolean(error)}
                    {...props}
                />
                {trailing}
            </span>
            {error && (
                <span role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                    {error}
                </span>
            )}
        </label>
    );
}

interface ModalProps {
    open: boolean;
    onClose: () => void;
    title: string;
    eyebrow?: string;
    children: ReactNode;
    compact?: boolean;
}

export function Modal({ open, onClose, title, eyebrow, children, compact = false }: ModalProps) {
    const [mounted, setMounted] = useState(open);
    const backdropRef = useRef<HTMLDivElement>(null);
    const panelRef = useRef<HTMLElement>(null);
    const closeRef = useRef(onClose);
    const previousFocusRef = useRef<HTMLElement | null>(null);
    const contentRef = useRef({ title, eyebrow, children });

    if (open) contentRef.current = { title, eyebrow, children };
    const modalContent = contentRef.current;

    useLayoutEffect(() => {
        closeRef.current = onClose;
    }, [onClose]);

    useLayoutEffect(() => {
        const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

        if (open) {
            setMounted(true);
            return;
        }

        if (!mounted) return;
        if (reduceMotion || !backdropRef.current || !panelRef.current) {
            setMounted(false);
            return;
        }

        const timeline = gsap.timeline({
            onComplete: () => setMounted(false),
            defaults: { overwrite: true },
        });
        timeline.to(panelRef.current, {
            y: -24,
            opacity: 0,
            duration: 0.2,
            ease: "power2.in",
        });
        timeline.to(
            backdropRef.current,
            { opacity: 0, duration: 0.16, ease: "power1.out" },
            "-=0.12",
        );

        return () => {
            timeline.kill();
        };
    }, [open, mounted]);

    useEffect(() => {
        if (!mounted || !open || !backdropRef.current || !panelRef.current) return;
        const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        if (reduceMotion) {
            gsap.set([backdropRef.current, panelRef.current], { clearProps: "all" });
            return;
        }

        const timeline = gsap.timeline({ defaults: { overwrite: true } });
        timeline.fromTo(
            backdropRef.current,
            { opacity: 0 },
            { opacity: 1, duration: 0.24, ease: "power1.out" },
        );
        timeline.fromTo(
            panelRef.current,
            { y: -28, opacity: 0, scale: 0.985 },
            { y: 0, opacity: 1, scale: 1, duration: 0.4, ease: "power3.out" },
            "-=0.16",
        );

        return () => {
            timeline.kill();
        };
    }, [mounted, open]);

    useEffect(() => {
        if (!open) return;
        previousFocusRef.current = document.activeElement as HTMLElement | null;
        const previousOverflow = document.body.style.overflow;
        document.body.style.overflow = "hidden";

        const keepFocusedFieldVisible = () => {
            window.requestAnimationFrame(() => {
                const activeElement = document.activeElement;
                if (
                    activeElement instanceof HTMLElement &&
                    activeElement.matches("input, textarea, select") &&
                    panelRef.current?.contains(activeElement)
                ) {
                    activeElement.scrollIntoView({ block: "nearest", behavior: "auto" });
                }
            });
        };
        window.visualViewport?.addEventListener("resize", keepFocusedFieldVisible, {
            passive: true,
        });

        const focusTimeout = window.setTimeout(() => {
            const firstFocusable =
                panelRef.current?.querySelector<HTMLElement>("[autofocus]") ??
                panelRef.current?.querySelector<HTMLElement>("input:not([disabled])") ??
                panelRef.current?.querySelector<HTMLElement>("select:not([disabled])") ??
                panelRef.current?.querySelector<HTMLElement>("textarea:not([disabled])") ??
                panelRef.current?.querySelector<HTMLElement>("button:not([disabled])") ??
                panelRef.current?.querySelector<HTMLElement>('[tabindex]:not([tabindex="-1"])');
            firstFocusable?.focus();
        }, 30);

        return () => {
            window.clearTimeout(focusTimeout);
            window.visualViewport?.removeEventListener("resize", keepFocusedFieldVisible);
            document.body.style.overflow = previousOverflow;
            previousFocusRef.current?.focus();
        };
    }, [open]);

    useEffect(() => {
        if (open) panelRef.current?.scrollTo({ top: 0, behavior: "auto" });
    }, [open, title]);

    useEffect(() => {
        if (!mounted) return;
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") closeRef.current();
            if (event.key !== "Tab" || !panelRef.current) return;

            const focusable = Array.from(
                panelRef.current.querySelectorAll<HTMLElement>(
                    'input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
                ),
            );
            if (focusable.length === 0) return;

            const first = focusable[0];
            const last = focusable[focusable.length - 1];
            if (event.shiftKey && document.activeElement === first) {
                event.preventDefault();
                last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
                event.preventDefault();
                first.focus();
            }
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [mounted]);

    if (!mounted) return null;

    return (
        <div
            ref={backdropRef}
            role="presentation"
            onMouseDown={(event) => {
                if (event.target === event.currentTarget) closeRef.current();
            }}
            css={css({
                position: "fixed",
                top: "var(--lumo-viewport-offset-top)",
                left: "var(--lumo-viewport-offset-left)",
                width: "var(--lumo-viewport-width)",
                height: "var(--lumo-viewport-height)",
                zIndex: 50,
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "center",
                padding:
                    "max(12px, var(--lumo-safe-top)) max(12px, var(--lumo-safe-right)) 12px max(12px, var(--lumo-safe-left))",
                background: "rgba(34, 28, 40, .38)",
                backdropFilter: "blur(8px)",
                "@media (min-width: 540px)": { alignItems: "center", padding: 24 },
            })}
        >
            <section
                ref={panelRef}
                role="dialog"
                aria-modal="true"
                aria-label={modalContent.title}
                css={css({
                    width: "min(100%, 460px)",
                    maxHeight:
                        "min(calc(var(--lumo-viewport-height) - max(24px, var(--lumo-safe-top))), 760px)",
                    overscrollBehavior: "contain",
                    overflowY: "auto",
                    scrollPadding: 16,
                    padding: compact ? "20px" : "24px 20px",
                    border: "1px solid rgba(255,255,255,.8)",
                    borderRadius: 26,
                    background: "#fff",
                    boxShadow: "0 20px 60px rgba(37,29,48,.2)",
                    "@media (min-width: 540px)": { borderRadius: 26, padding: compact ? 20 : 24 },
                    "@media (max-width: 340px)": {
                        padding: compact ? 16 : "20px 16px",
                    },
                    "@media (max-height: 480px)": {
                        paddingTop: 18,
                        paddingBottom: 18,
                    },
                })}
            >
                <header
                    css={css({
                        display: "flex",
                        alignItems: "flex-start",
                        justifyContent: "space-between",
                        gap: 16,
                        marginBottom: 20,
                    })}
                >
                    <div>
                        {modalContent.eyebrow && (
                            <p
                                css={css({
                                    marginBottom: 5,
                                    color: "var(--lumo-primary)",
                                    fontSize: 11,
                                    letterSpacing: ".09em",
                                    textTransform: "uppercase",
                                })}
                            >
                                {modalContent.eyebrow}
                            </p>
                        )}
                        <h2 css={css({ color: "var(--lumo-text)", fontSize: 21, lineHeight: 1.2 })}>
                            {modalContent.title}
                        </h2>
                    </div>
                    <IconButton label="Cerrar" icon={FiX} onClick={onClose} />
                </header>
                {modalContent.children}
            </section>
        </div>
    );
}

interface ToggleProps {
    checked: boolean;
    onChange: (checked: boolean) => void;
    label: string;
    description?: string;
    disabled?: boolean;
}

export function Toggle({ checked, onChange, label, description, disabled = false }: ToggleProps) {
    return (
        <label
            css={css({
                minHeight: 58,
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 16,
                cursor: disabled ? "default" : "pointer",
            })}
        >
            <span css={css({ display: "grid", gap: 3 })}>
                <span css={css({ color: "var(--lumo-text)", fontSize: 15 })}>{label}</span>
                {description && (
                    <span
                        css={css({
                            color: "var(--lumo-text-muted)",
                            fontSize: 12,
                            lineHeight: 1.4,
                        })}
                    >
                        {description}
                    </span>
                )}
            </span>
            <input
                type="checkbox"
                checked={checked}
                disabled={disabled}
                onChange={(event) => onChange(event.target.checked)}
                css={css({
                    width: 46,
                    height: 27,
                    flex: "0 0 auto",
                    appearance: "none",
                    border: "2px solid transparent",
                    borderRadius: 20,
                    background: checked ? "var(--lumo-primary)" : "#d9d4dc",
                    cursor: disabled ? "default" : "pointer",
                    transition: "background .2s ease",
                    "&::after": {
                        content: '""',
                        display: "block",
                        width: 21,
                        height: 21,
                        margin: 1,
                        borderRadius: "50%",
                        background: "#fff",
                        boxShadow: "0 2px 5px rgba(0,0,0,.18)",
                        transform: checked ? "translateX(19px)" : "translateX(0)",
                        transition: "transform .2s ease",
                    },
                })}
            />
        </label>
    );
}

interface ToastProps {
    title: string;
    detail?: string;
    onClose?: () => void;
}

export function Toast({ title, detail, onClose }: ToastProps) {
    useEffect(() => {
        if (!onClose) return;
        const timeout = window.setTimeout(onClose, 3200);
        return () => window.clearTimeout(timeout);
    }, [onClose, title, detail]);

    return (
        <div
            role="status"
            aria-live="polite"
            css={css({
                position: "fixed",
                zIndex: 70,
                left: "50%",
                bottom: "max(92px, calc(var(--lumo-safe-bottom) + 78px))",
                width: "min(calc(100% - 32px), 420px)",
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "space-between",
                gap: 12,
                padding: "14px 16px",
                border: "1px solid rgba(255,255,255,.14)",
                borderRadius: 17,
                color: "#fff",
                background: "rgba(45, 36, 56, .96)",
                boxShadow: "0 16px 40px rgba(34,26,44,.28)",
                animation: `${toastIn} .28s ease both`,
            })}
        >
            <span css={css({ display: "grid", gap: 3 })}>
                <strong css={css({ fontSize: 14, fontWeight: 500 })}>{title}</strong>
                {detail && <span css={css({ color: "#d8d2dc", fontSize: 12 })}>{detail}</span>}
            </span>
            {onClose && (
                <button
                    type="button"
                    aria-label="Cerrar aviso"
                    onClick={onClose}
                    css={css({
                        display: "grid",
                        placeItems: "center",
                        width: 30,
                        height: 30,
                        flex: "0 0 auto",
                        border: 0,
                        borderRadius: 10,
                        color: "#fff",
                        background: "rgba(255,255,255,.1)",
                        cursor: "pointer",
                    })}
                >
                    <FiX aria-hidden="true" />
                </button>
            )}
        </div>
    );
}

export function Pill({
    children,
    tone = "purple",
}: {
    children: ReactNode;
    tone?: "purple" | "green" | "amber" | "neutral";
}) {
    const colors = {
        purple: ["var(--lumo-lavender)", "var(--lumo-primary)"],
        green: ["var(--lumo-success-soft)", "var(--lumo-success)"],
        amber: ["var(--lumo-warning-soft)", "var(--lumo-warning)"],
        neutral: ["#efedf0", "var(--lumo-text-secondary)"],
    }[tone];

    return (
        <span
            css={css({
                minHeight: 26,
                width: "fit-content",
                maxWidth: "100%",
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "4px 10px",
                borderRadius: 999,
                color: colors[1],
                background: colors[0],
                fontSize: 11,
                lineHeight: 1.2,
                whiteSpace: "normal",
            })}
        >
            {children}
        </span>
    );
}
