import { lazy, Suspense, useEffect, useState } from "react";
import { css, keyframes } from "@emotion/react";
import { Toaster } from "sonner";

import { LumoProvider } from "@app/state/LumoProvider.tsx";
import { useLumo } from "@app/state/lumoContext.ts";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { Button, Modal } from "@shared/components/ui.tsx";

const GroupAccess = lazy(() => import("@modules/groups/GroupAccess.tsx"));
const Controller = lazy(() =>
    import("@modules/controller/Controller.tsx").then((module) => ({ default: module.Controller })),
);
const DebugLab = lazy(() =>
    import("@modules/debug/DebugLab.tsx").then((module) => ({ default: module.DebugLab })),
);
const ModeSelection = lazy(() =>
    import("@modules/onboarding/ModeSelection.tsx").then((module) => ({
        default: module.ModeSelection,
    })),
);
const TrackerSetup = lazy(() =>
    import("@modules/onboarding/TrackerSetup.tsx").then((module) => ({
        default: module.TrackerSetup,
    })),
);
const Tracker = lazy(() =>
    import("@modules/tracker/Tracker.tsx").then((module) => ({ default: module.Tracker })),
);

const reveal = keyframes({
    from: { opacity: 0, transform: "translateY(8px)" },
    to: { opacity: 1, transform: "translateY(0)" },
});

function Splash({
    error,
    onRetry,
    onReset,
}: {
    error?: string;
    onRetry?: () => void;
    onReset?: () => Promise<void>;
}) {
    const [confirmReset, setConfirmReset] = useState(false);
    const [resetting, setResetting] = useState(false);
    const [resetError, setResetError] = useState("");
    return (
        <main
            aria-label="Iniciando Lumo"
            css={css({
                position: "fixed",
                inset: 0,
                zIndex: 100,
                display: "grid",
                placeItems: "center",
                padding: 24,
                color: "var(--lumo-text)",
                background:
                    "radial-gradient(circle at 50% 38%, rgba(165,131,225,.22), transparent 36%), var(--lumo-bg)",
            })}
        >
            <div css={css({ display: "grid", justifyItems: "center", gap: 18 })}>
                <BrandMark size="large" animated />
                <div
                    css={css({
                        display: "grid",
                        justifyItems: "center",
                        gap: 5,
                        animation: `${reveal} .5s .75s ease both`,
                    })}
                >
                    <strong css={css({ fontSize: 28, letterSpacing: "-.04em" })}>lumo</strong>
                    <span css={css({ color: "var(--lumo-text-secondary)", fontSize: 13 })}>
                        Tu familia, un poco más cerca
                    </span>
                    {error && (
                        <div
                            css={css({
                                display: "grid",
                                gap: 14,
                                maxWidth: 320,
                                textAlign: "center",
                                marginTop: 16,
                            })}
                        >
                            <p
                                role="alert"
                                css={css({ color: "var(--lumo-text-secondary)", fontSize: 13 })}
                            >
                                {error}
                            </p>
                            <Button onClick={onRetry} disabled={resetting}>
                                Volver a intentar
                            </Button>
                            {onReset && (
                                <Button
                                    variant="secondary"
                                    onClick={() => setConfirmReset(true)}
                                    disabled={resetting}
                                >
                                    Vincular de nuevo
                                </Button>
                            )}
                        </div>
                    )}
                </div>
            </div>
            <Modal
                open={confirmReset}
                onClose={() => {
                    if (!resetting) setConfirmReset(false);
                }}
                eyebrow="Recuperar configuración"
                title="Vincular de nuevo"
            >
                <div css={css({ display: "grid", gap: 16 })}>
                    <p
                        css={css({
                            color: "var(--lumo-text-secondary)",
                            fontSize: 13,
                            lineHeight: 1.5,
                        })}
                    >
                        Se borrará el vínculo local y se detendrá el seguimiento de este teléfono.
                        Necesitarás un nuevo QR del supervisor con la opción de reconectar o
                        sustituir el teléfono controlado.
                    </p>
                    <p
                        css={css({
                            color: "var(--lumo-text-secondary)",
                            fontSize: 13,
                            lineHeight: 1.5,
                        })}
                    >
                        Si aún puedes recuperar la configuración desbloqueando el teléfono, vuelve
                        atrás y pulsa «Volver a intentar».
                    </p>
                    {resetError && (
                        <p role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                            {resetError}
                        </p>
                    )}
                    <Button
                        variant="danger"
                        loading={resetting}
                        onClick={async () => {
                            if (resetting || !onReset) return;
                            setResetting(true);
                            setResetError("");
                            try {
                                await onReset();
                                setConfirmReset(false);
                            } catch (requestError) {
                                setResetError(
                                    requestError instanceof Error
                                        ? requestError.message
                                        : "No se ha podido restablecer el vínculo local",
                                );
                            } finally {
                                setResetting(false);
                            }
                        }}
                    >
                        Borrar vínculo local
                    </Button>
                    <Button
                        variant="secondary"
                        disabled={resetting}
                        onClick={() => setConfirmReset(false)}
                    >
                        Volver atrás
                    </Button>
                </div>
            </Modal>
        </main>
    );
}

function AppContent() {
    const {
        state,
        dispatch,
        backend,
        bootstrapStatus,
        bootstrapError,
        bootstrapCanReset,
        retryBootstrap,
    } = useLumo();
    const [booting, setBooting] = useState(true);

    useEffect(() => {
        const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        const timeout = window.setTimeout(() => setBooting(false), reducedMotion ? 250 : 1550);
        return () => window.clearTimeout(timeout);
    }, []);

    if (booting) return <Splash />;
    if (bootstrapStatus !== "ready") {
        return (
            <Splash
                error={bootstrapStatus === "error" ? bootstrapError : undefined}
                onRetry={retryBootstrap}
                onReset={
                    bootstrapCanReset
                        ? async () => {
                              await backend.resetLocalSession();
                              retryBootstrap();
                          }
                        : undefined
                }
            />
        );
    }

    if (!state.group.active) {
        return (
            <GroupAccess
                onEnter={async (payload) => {
                    const snapshot =
                        payload.entry === "created"
                            ? await backend.createGroup(payload)
                            : await backend.joinGroup(
                                  payload.invitationId ?? "",
                                  payload.inviteToken ?? "",
                                  payload.pin,
                              );
                    if (backend.isNative() && !snapshot) {
                        throw new Error(
                            "La vinculación se está recuperando. Espera unos segundos antes de volver a intentarlo",
                        );
                    }
                    dispatch(
                        snapshot
                            ? { type: "HYDRATE_BACKEND", payload: snapshot }
                            : { type: "ENTER_GROUP", payload },
                    );
                }}
            />
        );
    }

    if (!state.mode) {
        return <ModeSelection onSelect={(mode) => dispatch({ type: "SET_MODE", payload: mode })} />;
    }

    if (state.mode === "tracker" && !state.preferences.trackerSetupComplete) {
        return <TrackerSetup />;
    }

    if (state.mode === "controller") return <Controller />;
    if (state.mode === "tracker") return <Tracker />;
    return <DebugLab />;
}

function App() {
    return (
        <>
            <Toaster
                position="top-center"
                swipeDirections={["top", "left", "right"]}
                visibleToasts={3}
                gap={8}
                offset={{ top: "calc(var(--lumo-safe-top) + 10px)" }}
                mobileOffset={{ top: "calc(var(--lumo-safe-top) + 10px)", left: 12, right: 12 }}
                toastOptions={{
                    style: {
                        border: "1px solid rgba(104,66,166,.16)",
                        borderRadius: 18,
                        color: "var(--lumo-text)",
                        background: "rgba(255,253,249,.97)",
                        boxShadow: "0 16px 38px rgba(47,38,57,.14)",
                        fontFamily: "inherit",
                    },
                    classNames: {
                        title: "lumo-toast-title",
                        description: "lumo-toast-description",
                        closeButton: "lumo-toast-close",
                    },
                }}
            />
            <LumoProvider>
                <div
                    css={css({
                        width: "min(100%, var(--lumo-viewport-width))",
                        minHeight: "var(--lumo-viewport-height)",
                        margin: "0 auto",
                        paddingLeft: "var(--lumo-safe-left)",
                        paddingRight: "var(--lumo-safe-right)",
                        background: "var(--lumo-bg)",
                        "@media (min-width: 540px)": {
                            maxWidth: 480,
                            boxShadow: "0 0 70px rgba(48, 35, 64, .15)",
                        },
                    })}
                >
                    <Suspense fallback={<Splash />}>
                        <AppContent />
                    </Suspense>
                </div>
            </LumoProvider>
        </>
    );
}

export default App;
