import { useState } from "react";
import { css, keyframes } from "@emotion/react";
import {
    FiAlertTriangle,
    FiBattery,
    FiCheck,
    FiHeart,
    FiHelpCircle,
    FiLock,
    FiLogOut,
    FiMapPin,
    FiPhone,
    FiSettings,
    FiShield,
    FiSmartphone,
    FiUserPlus,
    FiWifi,
} from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import {
    GroupSecurityModal,
    type GroupSecurityAction,
} from "@modules/groups/components/GroupSecurityModal.tsx";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { Button, Field, IconButton, Modal, Pill, Toast, Toggle } from "@shared/components/ui.tsx";
import { surface as card } from "@shared/styles/surfaces.ts";
import { formatRelative } from "@shared/utils/format.ts";

const breathe = keyframes({
    "0%, 100%": { transform: "scale(1)", boxShadow: "0 0 0 0 rgba(45,118,89,.18)" },
    "50%": { transform: "scale(1.035)", boxShadow: "0 0 0 15px rgba(45,118,89,0)" },
});

interface CheckRowProps {
    icon: IconType;
    title: string;
    detail: string;
    ok: boolean;
}

function CheckRow({ icon: Icon, title, detail, ok }: CheckRowProps) {
    return (
        <div
            css={css({
                display: "grid",
                gridTemplateColumns: "42px 1fr auto",
                alignItems: "center",
                gap: 11,
                padding: "12px 0",
                borderBottom: "1px solid var(--lumo-border)",
                "&:last-of-type": { borderBottom: 0 },
            })}
        >
            <span
                css={css({
                    width: 42,
                    height: 42,
                    display: "grid",
                    placeItems: "center",
                    borderRadius: 14,
                    color: ok ? "var(--lumo-success)" : "var(--lumo-warning)",
                    background: ok ? "var(--lumo-success-soft)" : "var(--lumo-warning-soft)",
                })}
            >
                <Icon size={18} />
            </span>
            <span css={css({ display: "grid", gap: 3 })}>
                <strong css={css({ fontSize: 13 })}>{title}</strong>
                <span css={css({ color: "var(--lumo-text-muted)", fontSize: 10, lineHeight: 1.4 })}>
                    {detail}
                </span>
            </span>
            <span
                aria-label={ok ? "Correcto" : "Requiere atención"}
                css={css({
                    width: 25,
                    height: 25,
                    display: "grid",
                    placeItems: "center",
                    borderRadius: 9,
                    color: "#fff",
                    background: ok ? "var(--lumo-success)" : "var(--lumo-warning)",
                })}
            >
                {ok ? <FiCheck size={15} /> : <FiAlertTriangle size={14} />}
            </span>
        </div>
    );
}

export function Tracker() {
    const { state, dispatch, backend } = useLumo();
    const [pinOpen, setPinOpen] = useState(false);
    const [settingsOpen, setSettingsOpen] = useState(false);
    const [helpOpen, setHelpOpen] = useState(false);
    const [securityAction, setSecurityAction] = useState<GroupSecurityAction | null>(null);
    const [pin, setPin] = useState("");
    const [pinError, setPinError] = useState("");
    const [unlocking, setUnlocking] = useState(false);
    const [toast, setToast] = useState<{ title: string; detail?: string } | null>(null);
    const permissionOk = state.mobile
        ? state.mobile.preciseLocation === "granted" &&
          state.mobile.backgroundLocation !== "denied" &&
          state.mobile.locationServicesEnabled
        : state.demo.permission === "granted";
    const trackingOk = state.mobile?.trackingEnabled ?? state.preferences.trackerSetupComplete;
    const connectionOk = state.demo.connection === "online";
    const batteryPercent = state.mobile?.batteryPercent ?? state.demo.battery;
    const batteryOk = batteryPercent > 15;
    const allGood = permissionOk && trackingOk && connectionOk && batteryOk;
    const supervisorName = state.group.supervisorName || "tu supervisor";
    const statusMessage = allGood
        ? `Tu ubicación se está compartiendo con ${supervisorName}.`
        : !trackingOk
          ? "El seguimiento está detenido. Actívalo desde los ajustes protegidos."
          : !permissionOk
            ? `La ubicación está desactivada. ${supervisorName} no puede ver tu posición actual.`
            : !connectionOk
              ? "No hay conexión disponible. Lumo lo intentará de nuevo automáticamente."
              : "La batería está baja. Conecta este teléfono para mantener la protección activa.";

    const unlock = async () => {
        setUnlocking(true);
        try {
            await backend.verifyPin(pin);
            setPinOpen(false);
            setPin("");
            setPinError("");
            window.setTimeout(() => setSettingsOpen(true), 220);
        } catch (requestError) {
            setPinError(
                requestError instanceof Error ? requestError.message : "El PIN no es correcto",
            );
        } finally {
            setUnlocking(false);
        }
    };

    const openSecurity = (action: GroupSecurityAction) => {
        setSettingsOpen(false);
        window.setTimeout(() => setSecurityAction(action), 220);
    };

    return (
        <main
            css={css({
                minHeight: "100dvh",
                display: "flex",
                flexDirection: "column",
                padding:
                    "max(18px, env(safe-area-inset-top)) 17px max(22px, env(safe-area-inset-bottom))",
                background:
                    "radial-gradient(circle at 50% -8%, rgba(165,131,225,.22), transparent 32%), var(--lumo-bg)",
            })}
        >
            <header
                css={css({
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    marginBottom: 28,
                })}
            >
                <div css={css({ display: "flex", alignItems: "center", gap: 10 })}>
                    <BrandMark size="small" />
                    <div css={css({ display: "grid", gap: 2 })}>
                        <strong css={css({ fontSize: 17, letterSpacing: "-.03em" })}>lumo</strong>
                        <span css={css({ color: "var(--lumo-text-muted)", fontSize: 9 })}>
                            Protección familiar
                        </span>
                    </div>
                </div>
                <IconButton
                    label="Abrir ajustes protegidos"
                    icon={FiSettings}
                    onClick={() => setPinOpen(true)}
                />
            </header>

            <section
                css={css({
                    display: "grid",
                    justifyItems: "center",
                    gap: 15,
                    padding: "28px 20px 24px",
                    border: `1px solid ${allGood ? "#cce3d6" : "#ead2b7"}`,
                    borderRadius: 28,
                    textAlign: "center",
                    background: allGood
                        ? "linear-gradient(155deg, #fff 0%, #edf7f1 100%)"
                        : "linear-gradient(155deg, #fff 0%, #fdf2e5 100%)",
                    boxShadow: "0 15px 34px rgba(47,38,57,.06)",
                })}
            >
                <span
                    css={css({
                        width: 76,
                        height: 76,
                        display: "grid",
                        placeItems: "center",
                        borderRadius: 26,
                        color: allGood ? "var(--lumo-success)" : "var(--lumo-warning)",
                        background: allGood
                            ? "var(--lumo-success-soft)"
                            : "var(--lumo-warning-soft)",
                        animation: allGood ? `${breathe} 3s ease-in-out infinite` : undefined,
                    })}
                >
                    {allGood ? <FiShield size={35} /> : <FiAlertTriangle size={34} />}
                </span>
                <div css={css({ display: "grid", gap: 7 })}>
                    <Pill tone={allGood ? "green" : "amber"}>
                        {allGood ? "Protección activa" : "Necesita atención"}
                    </Pill>
                    <h1
                        css={css({
                            fontSize: 26,
                            lineHeight: 1.12,
                            letterSpacing: "-.04em",
                        })}
                    >
                        {allGood
                            ? "Todo está en orden"
                            : batteryOk
                              ? "Lumo no puede compartir tu ubicación"
                              : "Este teléfono necesita batería"}
                    </h1>
                    <p
                        css={css({
                            maxWidth: 300,
                            color: "var(--lumo-text-secondary)",
                            fontSize: 12,
                            lineHeight: 1.55,
                        })}
                    >
                        {statusMessage}
                    </p>
                </div>
            </section>

            <section css={css(card, { marginTop: 16, padding: "3px 15px" })}>
                <CheckRow
                    icon={FiMapPin}
                    title="Ubicación"
                    detail={
                        permissionOk
                            ? trackingOk
                                ? "Permitida siempre"
                                : "Seguimiento detenido"
                            : "Revisa los permisos de Android"
                    }
                    ok={permissionOk}
                />
                <CheckRow
                    icon={FiWifi}
                    title="Conexión"
                    detail={connectionOk ? "Disponible" : "Sin conexión"}
                    ok={connectionOk}
                />
                <CheckRow
                    icon={FiSmartphone}
                    title="Última comprobación"
                    detail={formatRelative(state.demo.lastUpdatedAt)}
                    ok={connectionOk}
                />
            </section>

            <section
                css={css(card, {
                    display: "grid",
                    gridTemplateColumns: "auto 1fr auto",
                    alignItems: "center",
                    gap: 12,
                    marginTop: 12,
                    padding: 14,
                })}
            >
                <span
                    css={css({
                        width: 42,
                        height: 42,
                        display: "grid",
                        placeItems: "center",
                        borderRadius: 14,
                        color: batteryOk ? "var(--lumo-success)" : "var(--lumo-warning)",
                        background: batteryOk
                            ? "var(--lumo-success-soft)"
                            : "var(--lumo-warning-soft)",
                    })}
                >
                    <FiBattery size={19} />
                </span>
                <span css={css({ display: "grid", gap: 7 })}>
                    <span css={css({ fontSize: 12 })}>Batería de este teléfono</span>
                    <span
                        css={css({
                            height: 6,
                            overflow: "hidden",
                            borderRadius: 8,
                            background: "#e7e2e8",
                            "&::after": {
                                content: '""',
                                display: "block",
                                width: `${batteryPercent}%`,
                                height: "100%",
                                borderRadius: 8,
                                background: batteryOk
                                    ? "var(--lumo-success)"
                                    : "var(--lumo-warning)",
                            },
                        })}
                    />
                </span>
                <strong css={css({ fontSize: 14 })}>{batteryPercent} %</strong>
            </section>

            <Button
                fullWidth
                icon={FiHelpCircle}
                css={css({ marginTop: "auto", minHeight: 58 })}
                onClick={() => setHelpOpen(true)}
            >
                Necesito ayuda
            </Button>
            <p
                css={css({
                    marginTop: 10,
                    color: "var(--lumo-text-muted)",
                    textAlign: "center",
                    fontSize: 10,
                })}
            >
                Lumo permanece visible para que siempre sepas cuándo está activo.
            </p>

            <Modal
                open={pinOpen}
                onClose={() => setPinOpen(false)}
                eyebrow="Ajustes protegidos"
                title="Introduce el PIN familiar"
            >
                <div css={css({ display: "grid", gap: 15 })}>
                    <p
                        css={css({
                            color: "var(--lumo-text-secondary)",
                            fontSize: 12,
                            lineHeight: 1.5,
                        })}
                    >
                        Este control evita cambios accidentales dentro de la app. Usa el PIN de 6
                        cifras del grupo.
                    </p>
                    <Field
                        type="password"
                        inputMode="numeric"
                        autoFocus
                        maxLength={6}
                        label="PIN de 6 cifras"
                        placeholder="••••••"
                        icon={FiLock}
                        value={pin}
                        error={pinError}
                        onChange={(event) => {
                            setPin(event.target.value.replace(/\D/g, ""));
                            setPinError("");
                        }}
                        onKeyDown={(event) => {
                            if (event.key === "Enter") unlock();
                        }}
                    />
                    <Button
                        fullWidth
                        icon={FiLock}
                        disabled={pin.length !== 6}
                        loading={unlocking}
                        onClick={unlock}
                    >
                        Abrir ajustes
                    </Button>
                </div>
            </Modal>

            <Modal
                open={settingsOpen}
                onClose={() => setSettingsOpen(false)}
                eyebrow="Teléfono acompañado"
                title="Ajustes de protección"
            >
                <div css={css({ display: "grid", gap: 13 })}>
                    <article css={css(card, { padding: "4px 14px" })}>
                        <Toggle
                            label="Seguimiento activo"
                            description={
                                trackingOk
                                    ? "El servicio de ubicación está funcionando"
                                    : "El servicio de ubicación está detenido"
                            }
                            checked={trackingOk}
                            onChange={async (checked) => {
                                try {
                                    const result = await backend.setControlledTracking(checked);
                                    if (result.status) {
                                        dispatch({
                                            type: "SYNC_MOBILE_STATUS",
                                            payload: result.status,
                                        });
                                    }
                                    if (result.snapshot) {
                                        dispatch({
                                            type: "HYDRATE_BACKEND",
                                            payload: result.snapshot,
                                        });
                                    }
                                } catch (requestError) {
                                    setToast({
                                        title: "No se ha podido cambiar",
                                        detail:
                                            requestError instanceof Error
                                                ? requestError.message
                                                : "Revisa los permisos de Android",
                                    });
                                }
                            }}
                        />
                        <div css={css({ height: 1, background: "var(--lumo-border)" })} />
                        <Toggle
                            label="Protección de batería"
                            description="Evita que Android detenga el seguimiento"
                            checked={state.mobile?.batteryOptimizationDisabled ?? false}
                            onChange={async () => {
                                try {
                                    await backend.openBatterySettings();
                                } catch (requestError) {
                                    setToast({
                                        title: "No se han podido abrir los ajustes",
                                        detail:
                                            requestError instanceof Error
                                                ? requestError.message
                                                : "Inténtalo de nuevo",
                                    });
                                }
                            }}
                        />
                    </article>
                    {state.group.role === "supervisor" && (
                        <>
                            <Button
                                variant="secondary"
                                fullWidth
                                icon={FiSettings}
                                onClick={() => dispatch({ type: "SET_MODE", payload: null })}
                            >
                                Elegir otro modo
                            </Button>
                            <Button
                                variant="secondary"
                                fullWidth
                                icon={FiUserPlus}
                                onClick={() => openSecurity("invite")}
                            >
                                Invitar a un miembro
                            </Button>
                        </>
                    )}
                    <Button
                        variant="danger"
                        fullWidth
                        icon={FiLogOut}
                        onClick={() => openSecurity("leave")}
                    >
                        Desvincular este teléfono
                    </Button>
                    <p
                        css={css({
                            color: "var(--lumo-text-muted)",
                            fontSize: 10,
                            lineHeight: 1.5,
                        })}
                    >
                        El PIN solo protege esta interfaz. No puede bloquear los ajustes ni los
                        permisos de Android.
                    </p>
                </div>
            </Modal>

            <Modal
                open={helpOpen}
                onClose={() => setHelpOpen(false)}
                eyebrow="Ayuda familiar"
                title={`¿Quieres avisar a ${supervisorName}?`}
            >
                <div
                    css={css({
                        display: "grid",
                        justifyItems: "center",
                        gap: 16,
                        textAlign: "center",
                    })}
                >
                    <span
                        css={css({
                            width: 72,
                            height: 72,
                            display: "grid",
                            placeItems: "center",
                            borderRadius: 24,
                            color: "var(--lumo-danger)",
                            background: "var(--lumo-danger-soft)",
                        })}
                    >
                        <FiHeart size={31} />
                    </span>
                    <p
                        css={css({
                            color: "var(--lumo-text-secondary)",
                            fontSize: 13,
                            lineHeight: 1.55,
                        })}
                    >
                        {supervisorName} recibirá un aviso prioritario con la última ubicación
                        disponible.
                    </p>
                    <Button
                        fullWidth
                        icon={FiHeart}
                        onClick={async () => {
                            try {
                                const snapshot = await backend.sendHelp();
                                if (snapshot)
                                    dispatch({ type: "HYDRATE_BACKEND", payload: snapshot });
                                setHelpOpen(false);
                                setToast({
                                    title: "Aviso enviado",
                                    detail: `${supervisorName} recibirá tu solicitud de ayuda`,
                                });
                            } catch (requestError) {
                                setToast({
                                    title: "No se ha podido enviar",
                                    detail:
                                        requestError instanceof Error
                                            ? requestError.message
                                            : "Inténtalo de nuevo",
                                });
                            }
                        }}
                    >
                        Avisar a {supervisorName}
                    </Button>
                    <Button
                        variant="secondary"
                        fullWidth
                        icon={FiPhone}
                        disabled={!state.group.supervisorPhone}
                        onClick={async () => {
                            try {
                                await backend.openPhoneDialer(state.group.supervisorPhone);
                            } catch (requestError) {
                                setToast({
                                    title: "No se ha podido preparar la llamada",
                                    detail:
                                        requestError instanceof Error
                                            ? requestError.message
                                            : "Revisa el número configurado",
                                });
                            }
                        }}
                    >
                        Llamar a {supervisorName}
                    </Button>
                </div>
            </Modal>

            <GroupSecurityModal action={securityAction} onClose={() => setSecurityAction(null)} />

            {toast && (
                <Toast title={toast.title} detail={toast.detail} onClose={() => setToast(null)} />
            )}
        </main>
    );
}
