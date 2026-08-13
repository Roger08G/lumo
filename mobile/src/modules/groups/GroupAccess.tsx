import { useState, type FormEvent } from "react";
import { css, keyframes } from "@emotion/react";
import {
    FiCamera,
    FiCheck,
    FiChevronLeft,
    FiHeart,
    FiLock,
    FiShield,
    FiUser,
    FiUsers,
} from "react-icons/fi";

import { GroupButtons } from "@modules/groups/components/GroupButtons.tsx";
import { useLumo } from "@app/state/lumoContext.ts";
import { BrandMark } from "@shared/components/BrandMark.tsx";
import { StepProgress } from "@shared/components/StepProgress.tsx";
import { Button, Field, Modal, Pill } from "@shared/components/ui.tsx";
import type { GroupEntryPayload } from "@shared/types/lumo.ts";

const enter = keyframes({
    from: { opacity: 0, transform: "translateY(12px)" },
    to: { opacity: 1, transform: "translateY(0)" },
});

const scan = keyframes({
    "0%": { top: "17%", opacity: 0 },
    "12%": { opacity: 1 },
    "88%": { opacity: 1 },
    "100%": { top: "79%", opacity: 0 },
});

interface GroupAccessProps {
    onEnter: (payload: GroupEntryPayload) => Promise<void> | void;
}

interface PreviewInvite {
    version: 1;
    kind: "lumo-group-invite";
    name: string;
    code: string;
    supervisorName?: string;
    trackedPersonName?: string;
    token?: string;
}

type GroupAction = "create" | "join" | null;
type CreateStep = 0 | 1 | 2 | 3;
type JoinStep = "scan" | "pin";

function readPreviewInvite(): PreviewInvite {
    try {
        const stored = JSON.parse(
            localStorage.getItem("lumo.preview-invite") ?? "null",
        ) as PreviewInvite | null;
        if (
            stored?.version === 1 &&
            stored.kind === "lumo-group-invite" &&
            stored.name &&
            stored.code
        ) {
            return stored;
        }
    } catch {
        // The visual demo can fall back to a sample invitation.
    }

    return {
        version: 1,
        kind: "lumo-group-invite",
        name: "Grupo familiar",
        code: "LUMO24",
        supervisorName: "Supervisor",
        trackedPersonName: "Persona acompañada",
        token: "preview-invitation",
    };
}

export default function GroupAccess({ onEnter }: GroupAccessProps) {
    const { backend } = useLumo();
    const [action, setAction] = useState<GroupAction>(null);
    const [createStep, setCreateStep] = useState<CreateStep>(0);
    const [joinStep, setJoinStep] = useState<JoinStep>("scan");
    const [groupName, setGroupName] = useState("");
    const [userName, setUserName] = useState("");
    const [trackedPersonName, setTrackedPersonName] = useState("");
    const [groupPin, setGroupPin] = useState("");
    const [scannedInvite, setScannedInvite] = useState<PreviewInvite | null>(null);
    const [error, setError] = useState("");
    const [loading, setLoading] = useState(false);

    const resetFlow = () => {
        setCreateStep(0);
        setJoinStep("scan");
        setGroupName("");
        setUserName("");
        setTrackedPersonName("");
        setGroupPin("");
        setScannedInvite(null);
        setError("");
        setLoading(false);
    };

    const close = () => {
        setAction(null);
        resetFlow();
    };

    const begin = (nextAction: Exclude<GroupAction, null>) => {
        resetFlow();
        setAction(nextAction);
    };

    const submit = async (event: FormEvent) => {
        event.preventDefault();
        setError("");

        if (action === "create") {
            if (createStep === 0) {
                if (groupName.trim().length < 2) {
                    setError("Escribe un nombre para el grupo");
                    return;
                }
                setCreateStep(1);
                return;
            }

            if (createStep === 1) {
                if (userName.trim().length < 2) {
                    setError("Escribe tu nombre");
                    return;
                }
                setCreateStep(2);
                return;
            }

            if (createStep === 2) {
                if (trackedPersonName.trim().length < 2) {
                    setError("Escribe el nombre de la persona acompañada");
                    return;
                }
                setCreateStep(3);
                return;
            }

            if (!/^\d{6}$/.test(groupPin)) {
                setError("El PIN debe tener exactamente 6 cifras");
                return;
            }

            setLoading(true);
            try {
                await onEnter({
                    name: groupName.trim(),
                    code: "",
                    pin: groupPin,
                    userName: userName.trim(),
                    supervisorName: userName.trim(),
                    trackedPersonName: trackedPersonName.trim(),
                    role: "supervisor",
                    entry: "created",
                });
            } catch (requestError) {
                setError(
                    requestError instanceof Error
                        ? requestError.message
                        : "No se ha podido crear el grupo",
                );
                setLoading(false);
            }
            return;
        }

        if (!scannedInvite) {
            setError("Escanea primero una invitación válida");
            return;
        }
        if (!/^\d{6}$/.test(groupPin)) {
            setError("Introduce las 6 cifras del PIN");
            return;
        }
        setLoading(true);
        try {
            await onEnter({
                name: scannedInvite.name,
                code: scannedInvite.code,
                pin: groupPin,
                userName: scannedInvite.trackedPersonName || "Miembro",
                supervisorName: scannedInvite.supervisorName || "Supervisor",
                trackedPersonName: scannedInvite.trackedPersonName || "Persona acompañada",
                role: "member",
                entry: "joined",
                inviteToken: scannedInvite.token,
            });
        } catch (requestError) {
            setError(
                requestError instanceof Error
                    ? requestError.message
                    : "No se ha podido unir al grupo",
            );
            setLoading(false);
        }
    };

    const simulateScan = async () => {
        if (loading) return;
        setLoading(true);
        setError("");
        try {
            const scanned = await backend.scanInvitation();
            if (!scanned) await new Promise((resolve) => window.setTimeout(resolve, 850));
            setScannedInvite((scanned as PreviewInvite | null) ?? readPreviewInvite());
            setJoinStep("pin");
        } catch (requestError) {
            setError(
                requestError instanceof Error
                    ? requestError.message
                    : "No se ha podido leer el código QR",
            );
        } finally {
            setLoading(false);
        }
    };

    const createTitle = [
        "Ponle un nombre",
        "Crea tu perfil",
        "¿A quién acompañas?",
        "Define el PIN",
    ][createStep];

    return (
        <main
            css={css({
                minHeight: "100dvh",
                display: "flex",
                flexDirection: "column",
                justifyContent: "center",
                alignItems: "center",
                padding:
                    "max(28px, env(safe-area-inset-top)) 24px max(28px, env(safe-area-inset-bottom))",
                background: "var(--lumo-bg)",
            })}
        >
            <section
                css={css({
                    width: "100%",
                    maxWidth: 360,
                    minHeight: "min(660px, calc(100dvh - 56px))",
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "center",
                })}
            >
                <div
                    css={css({
                        display: "grid",
                        justifyItems: "center",
                        gap: 18,
                        marginTop: "clamp(72px, 18dvh, 148px)",
                        textAlign: "center",
                        animation: `${enter} .45s ease both`,
                    })}
                >
                    <BrandMark size="large" animated />
                    <div css={css({ display: "grid", justifyItems: "center", gap: 6 })}>
                        <h1
                            css={css({
                                color: "var(--lumo-text)",
                                fontSize: 30,
                                lineHeight: "32px",
                                letterSpacing: "-.045em",
                            })}
                        >
                            lumo
                        </h1>
                        <p
                            css={css({
                                color: "var(--lumo-text-secondary)",
                                fontSize: 14,
                                lineHeight: "20px",
                            })}
                        >
                            Tu familia, un poco más cerca.
                        </p>
                    </div>
                </div>

                <div
                    css={css({
                        width: "100%",
                        marginTop: "auto",
                        animation: `${enter} .3s .12s ease both`,
                    })}
                >
                    <GroupButtons onCreate={() => begin("create")} onJoin={() => begin("join")} />
                </div>
            </section>

            <Modal
                open={action === "create"}
                onClose={close}
                eyebrow={`Paso ${createStep + 1} de 4`}
                title={createTitle}
            >
                <form onSubmit={submit} css={css({ display: "grid", gap: 17 })}>
                    <StepProgress current={createStep} total={4} />
                    {createStep === 0 && (
                        <Field
                            key="group-name"
                            autoFocus
                            label="Nombre del grupo"
                            placeholder="Nombre del grupo"
                            icon={FiUsers}
                            value={groupName}
                            onChange={(event) => {
                                setGroupName(event.target.value);
                                setError("");
                            }}
                        />
                    )}
                    {createStep === 1 && (
                        <Field
                            key="user-name"
                            autoFocus
                            label="Tu nombre"
                            placeholder="Tu nombre"
                            icon={FiUser}
                            value={userName}
                            onChange={(event) => {
                                setUserName(event.target.value);
                                setError("");
                            }}
                        />
                    )}
                    {createStep === 2 && (
                        <Field
                            key="tracked-person-name"
                            autoFocus
                            label="Nombre de la persona acompañada"
                            placeholder="Nombre de la persona"
                            icon={FiHeart}
                            value={trackedPersonName}
                            onChange={(event) => {
                                setTrackedPersonName(event.target.value);
                                setError("");
                            }}
                        />
                    )}
                    {createStep === 3 && (
                        <div key="group-pin" css={css({ display: "grid", gap: 12 })}>
                            <div
                                css={css({
                                    display: "flex",
                                    alignItems: "center",
                                    gap: 10,
                                    color: "var(--lumo-text-secondary)",
                                    fontSize: 12,
                                    lineHeight: 1.45,
                                })}
                            >
                                <FiShield size={19} css={css({ color: "var(--lumo-primary)" })} />
                                Autoriza invitaciones y cambios importantes.
                            </div>
                            <Field
                                autoFocus
                                type="password"
                                inputMode="numeric"
                                autoComplete="new-password"
                                label="PIN de 6 cifras"
                                placeholder="••••••"
                                icon={FiLock}
                                maxLength={6}
                                value={groupPin}
                                onChange={(event) => {
                                    setGroupPin(event.target.value.replace(/\D/g, "").slice(0, 6));
                                    setError("");
                                }}
                            />
                        </div>
                    )}
                    {error && (
                        <p role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                            {error}
                        </p>
                    )}
                    <div
                        css={css({
                            display: "grid",
                            gridTemplateColumns: createStep === 0 ? "1fr" : "104px 1fr",
                            gap: 10,
                        })}
                    >
                        {createStep > 0 && (
                            <Button
                                type="button"
                                variant="secondary"
                                icon={FiChevronLeft}
                                onClick={() => {
                                    setCreateStep((createStep - 1) as CreateStep);
                                    setError("");
                                }}
                            >
                                Atrás
                            </Button>
                        )}
                        <Button
                            type="submit"
                            fullWidth
                            loading={loading}
                            disabled={
                                (createStep === 0 && groupName.trim().length < 2) ||
                                (createStep === 1 && userName.trim().length < 2) ||
                                (createStep === 2 && trackedPersonName.trim().length < 2) ||
                                (createStep === 3 && groupPin.length !== 6)
                            }
                        >
                            {createStep === 3 ? "Crear como supervisor" : "Continuar"}
                        </Button>
                    </div>
                </form>
            </Modal>

            <Modal
                open={action === "join"}
                onClose={close}
                eyebrow={joinStep === "scan" ? "Invitación segura" : "QR verificado"}
                title={joinStep === "scan" ? "Escanea el código QR" : "Introduce el PIN"}
            >
                {joinStep === "scan" ? (
                    <div css={css({ display: "grid", gap: 16 })}>
                        <div
                            aria-label="Escáner QR de demostración"
                            css={css({
                                position: "relative",
                                height: 224,
                                overflow: "hidden",
                                border: "1px solid var(--lumo-border-strong)",
                                borderRadius: 24,
                                background:
                                    "radial-gradient(circle at 50% 45%, rgba(255,255,255,.96), rgba(238,231,248,.82)), var(--lumo-lavender)",
                            })}
                        >
                            <div
                                css={css({
                                    position: "absolute",
                                    inset: 37,
                                    border: "2px solid rgba(104,66,166,.34)",
                                    borderRadius: 22,
                                })}
                            />
                            <span
                                css={css({
                                    position: "absolute",
                                    zIndex: 2,
                                    top: "17%",
                                    right: 42,
                                    left: 42,
                                    height: 2,
                                    borderRadius: 999,
                                    background: "var(--lumo-primary)",
                                    boxShadow: "0 0 12px rgba(104,66,166,.6)",
                                    animation: `${scan} 2.2s ease-in-out infinite`,
                                })}
                            />
                            <span
                                css={css({
                                    position: "absolute",
                                    inset: 0,
                                    display: "grid",
                                    placeItems: "center",
                                    color: "var(--lumo-primary)",
                                })}
                            >
                                <FiCamera size={36} />
                            </span>
                            <span css={css({ position: "absolute", right: 12, bottom: 12 })}>
                                <Pill tone="neutral">Demo visual</Pill>
                            </span>
                        </div>
                        <p
                            css={css({
                                color: "var(--lumo-text-secondary)",
                                fontSize: 12,
                                lineHeight: 1.5,
                                textAlign: "center",
                            })}
                        >
                            Apunta al QR que muestra el supervisor del grupo.
                        </p>
                        {error && (
                            <p
                                role="alert"
                                css={css({ color: "var(--lumo-danger)", fontSize: 12 })}
                            >
                                {error}
                            </p>
                        )}
                        <Button fullWidth icon={FiCamera} loading={loading} onClick={simulateScan}>
                            {loading
                                ? "Leyendo código…"
                                : backend.isMobileNative()
                                  ? "Escanear código QR"
                                  : "Simular escaneo válido"}
                        </Button>
                    </div>
                ) : (
                    <form onSubmit={submit} css={css({ display: "grid", gap: 16 })}>
                        <div
                            css={css({
                                display: "flex",
                                alignItems: "center",
                                gap: 11,
                                padding: 13,
                                border: "1px solid #b9dac8",
                                borderRadius: 17,
                                color: "var(--lumo-success)",
                                background: "var(--lumo-success-soft)",
                            })}
                        >
                            <FiCheck size={20} />
                            <div css={css({ display: "grid", gap: 2 })}>
                                <strong css={css({ fontSize: 13 })}>{scannedInvite?.name}</strong>
                                <span css={css({ fontSize: 10 })}>Código QR válido</span>
                            </div>
                        </div>
                        <Field
                            key="join-pin"
                            autoFocus
                            type="password"
                            inputMode="numeric"
                            autoComplete="one-time-code"
                            label="PIN proporcionado por el supervisor"
                            placeholder="••••••"
                            icon={FiLock}
                            maxLength={6}
                            value={groupPin}
                            onChange={(event) => {
                                setGroupPin(event.target.value.replace(/\D/g, "").slice(0, 6));
                                setError("");
                            }}
                        />
                        {error && (
                            <p
                                role="alert"
                                css={css({ color: "var(--lumo-danger)", fontSize: 12 })}
                            >
                                {error}
                            </p>
                        )}
                        <div
                            css={css({
                                display: "grid",
                                gridTemplateColumns: "104px 1fr",
                                gap: 10,
                            })}
                        >
                            <Button
                                type="button"
                                variant="secondary"
                                icon={FiChevronLeft}
                                onClick={() => {
                                    setJoinStep("scan");
                                    setGroupPin("");
                                    setError("");
                                }}
                            >
                                Atrás
                            </Button>
                            <Button
                                type="submit"
                                fullWidth
                                loading={loading}
                                disabled={groupPin.length !== 6}
                            >
                                Unirme al grupo
                            </Button>
                        </div>
                    </form>
                )}
            </Modal>
        </main>
    );
}
