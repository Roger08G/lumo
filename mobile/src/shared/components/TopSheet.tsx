import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { css } from "@emotion/react";
import gsap from "gsap";
import { FiX } from "react-icons/fi";

import { IconButton } from "@shared/components/ui.tsx";

interface TopSheetProps {
    open: boolean;
    onClose: () => void;
    title: string;
    eyebrow?: string;
    children: ReactNode;
}

export function TopSheet({ open, onClose, title, eyebrow, children }: TopSheetProps) {
    const [mounted, setMounted] = useState(open);
    const backdropRef = useRef<HTMLDivElement>(null);
    const panelRef = useRef<HTMLElement>(null);
    const closeRef = useRef(onClose);
    const previousFocusRef = useRef<HTMLElement | null>(null);
    const contentRef = useRef({ title, eyebrow, children });

    if (open) contentRef.current = { title, eyebrow, children };
    const content = contentRef.current;

    useLayoutEffect(() => {
        closeRef.current = onClose;
    }, [onClose]);

    useLayoutEffect(() => {
        if (open) {
            setMounted(true);
            return;
        }
        if (!mounted) return;

        const backdrop = backdropRef.current;
        const panel = panelRef.current;
        const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        if (!backdrop || !panel || reduceMotion) {
            setMounted(false);
            return;
        }

        const timeline = gsap.timeline({
            defaults: { overwrite: true },
            onComplete: () => setMounted(false),
        });
        timeline.to(panel, {
            yPercent: -105,
            opacity: 0.72,
            duration: 0.3,
            ease: "power3.in",
        });
        timeline.to(backdrop, { opacity: 0, duration: 0.18, ease: "power1.out" }, "-=0.18");

        return () => {
            timeline.kill();
        };
    }, [mounted, open]);

    useEffect(() => {
        if (!mounted || !open) return;
        const backdrop = backdropRef.current;
        const panel = panelRef.current;
        if (!backdrop || !panel) return;

        const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        if (reduceMotion) {
            gsap.set([backdrop, panel], { clearProps: "all" });
            return;
        }

        const timeline = gsap.timeline({ defaults: { overwrite: true } });
        timeline.fromTo(
            backdrop,
            { opacity: 0 },
            { opacity: 1, duration: 0.24, ease: "power1.out" },
        );
        timeline.fromTo(
            panel,
            { yPercent: -105, opacity: 0.72 },
            { yPercent: 0, opacity: 1, duration: 0.5, ease: "power4.out" },
            "-=0.18",
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

        const focusTimeout = window.setTimeout(() => {
            panelRef.current?.querySelector<HTMLElement>("button:not([disabled])")?.focus();
        }, 80);

        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                closeRef.current();
                return;
            }
            if (event.key !== "Tab" || !panelRef.current) return;

            const focusable = Array.from(
                panelRef.current.querySelectorAll<HTMLElement>(
                    'button:not([disabled]), a[href], input:not([disabled]), [tabindex]:not([tabindex="-1"])',
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

        return () => {
            window.clearTimeout(focusTimeout);
            window.removeEventListener("keydown", onKeyDown);
            document.body.style.overflow = previousOverflow;
            previousFocusRef.current?.focus();
        };
    }, [open]);

    useEffect(() => {
        if (open) panelRef.current?.scrollTo({ top: 0, behavior: "auto" });
    }, [open, title]);

    if (!mounted) return null;

    return (
        <div
            ref={backdropRef}
            role="presentation"
            onPointerDown={(event) => {
                if (event.target === event.currentTarget) closeRef.current();
            }}
            css={css({
                position: "fixed",
                top: "var(--lumo-viewport-offset-top)",
                left: "var(--lumo-viewport-offset-left)",
                zIndex: 60,
                width: "var(--lumo-viewport-width)",
                height: "var(--lumo-viewport-height)",
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "center",
                padding: "0 max(0px, var(--lumo-safe-right)) 0 max(0px, var(--lumo-safe-left))",
                background: "rgba(34,28,40,.34)",
                backdropFilter: "blur(7px)",
            })}
        >
            <section
                ref={panelRef}
                role="dialog"
                aria-modal="true"
                aria-label={content.title}
                css={css({
                    width: "min(100%, 480px)",
                    maxHeight:
                        "min(calc(var(--lumo-viewport-height) - max(16px, var(--lumo-safe-bottom))), 720px)",
                    overflowY: "auto",
                    overscrollBehavior: "contain",
                    padding:
                        "max(18px, var(--lumo-safe-top)) 18px max(22px, var(--lumo-safe-bottom))",
                    border: "1px solid rgba(255,255,255,.82)",
                    borderTop: 0,
                    borderRadius: "0 0 28px 28px",
                    background: "rgba(255,255,255,.98)",
                    boxShadow: "0 22px 56px rgba(37,29,48,.22)",
                    scrollbarWidth: "thin",
                    "@media (min-width: 540px)": {
                        marginTop: "max(12px, var(--lumo-safe-top))",
                        maxHeight:
                            "min(calc(var(--lumo-viewport-height) - max(36px, var(--lumo-safe-top)) - max(20px, var(--lumo-safe-bottom))), 720px)",
                        padding: 22,
                        borderTop: "1px solid rgba(255,255,255,.82)",
                        borderRadius: 28,
                    },
                    "@media (max-width: 340px)": {
                        paddingLeft: 15,
                        paddingRight: 15,
                    },
                })}
            >
                <header
                    css={css({
                        display: "flex",
                        alignItems: "flex-start",
                        justifyContent: "space-between",
                        gap: 16,
                        marginBottom: 18,
                    })}
                >
                    <div css={css({ minWidth: 0 })}>
                        {content.eyebrow && (
                            <p
                                css={css({
                                    marginBottom: 5,
                                    color: "var(--lumo-primary)",
                                    fontSize: 10,
                                    fontWeight: 500,
                                    letterSpacing: ".1em",
                                    textTransform: "uppercase",
                                })}
                            >
                                {content.eyebrow}
                            </p>
                        )}
                        <h2
                            css={css({
                                overflow: "hidden",
                                color: "var(--lumo-text)",
                                fontSize: 21,
                                lineHeight: 1.2,
                                textOverflow: "ellipsis",
                            })}
                        >
                            {content.title}
                        </h2>
                    </div>
                    <IconButton label="Cerrar avisos" icon={FiX} onClick={closeRef.current} />
                </header>
                {content.children}
            </section>
        </div>
    );
}
