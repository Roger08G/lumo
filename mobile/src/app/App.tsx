import { useEffect, useState } from "react";
import { css, keyframes } from "@emotion/react";

import { LumoProvider } from "@app/state/LumoProvider.tsx";
import { useLumo } from "@app/state/lumoContext.ts";
import GroupAccess from "@modules/groups/GroupAccess.tsx";
import { Controller } from "@modules/controller/Controller.tsx";
import { DebugLab } from "@modules/debug/DebugLab.tsx";
import { ModeSelection } from "@modules/onboarding/ModeSelection.tsx";
import { TrackerSetup } from "@modules/onboarding/TrackerSetup.tsx";
import { Tracker } from "@modules/tracker/Tracker.tsx";
import { BrandMark } from "@shared/components/BrandMark.tsx";

const reveal = keyframes({
    from: { opacity: 0, transform: "translateY(8px)" },
    to: { opacity: 1, transform: "translateY(0)" },
});

function Splash() {
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
                </div>
            </div>
        </main>
    );
}

function AppContent() {
    const { state, dispatch, backend } = useLumo();
    const [booting, setBooting] = useState(true);

    useEffect(() => {
        const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        const timeout = window.setTimeout(() => setBooting(false), reducedMotion ? 250 : 1550);
        return () => window.clearTimeout(timeout);
    }, []);

    if (booting) return <Splash />;

    if (!state.group.active) {
        return (
            <GroupAccess
                onEnter={async (payload) => {
                    const snapshot =
                        payload.role === "supervisor"
                            ? await backend.createGroup(payload)
                            : await backend.joinGroup(payload.inviteToken ?? "", payload.pin);
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
        <LumoProvider>
            <div
                css={css({
                    width: "100%",
                    minHeight: "100dvh",
                    margin: "0 auto",
                    background: "var(--lumo-bg)",
                    "@media (min-width: 540px)": {
                        maxWidth: 480,
                        boxShadow: "0 0 70px rgba(48, 35, 64, .15)",
                    },
                })}
            >
                <AppContent />
            </div>
        </LumoProvider>
    );
}

export default App;
