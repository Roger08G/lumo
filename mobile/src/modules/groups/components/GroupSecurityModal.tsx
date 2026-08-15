import { useEffect, useState, type FormEvent } from "react";
import { css } from "@emotion/react";
import {
    FiCheck,
    FiCopy,
    FiKey,
    FiLock,
    FiLogOut,
    FiMapPin,
    FiShield,
    FiUserPlus,
} from "react-icons/fi";
import { QRCodeSVG } from "qrcode.react";

import { useLumo } from "@app/state/lumoContext.ts";
import { Button, Field, Modal, Pill } from "@shared/components/ui.tsx";
import type { InvitationData } from "@shared/services/lumoBackend.ts";
import lumoLogo from "@tauri/icons/icon.png";

export type GroupSecurityAction = "invite" | "leave";

interface GroupSecurityModalProps {
    action: GroupSecurityAction | null;
    onClose: () => void;
}

export function GroupSecurityModal({ action, onClose }: GroupSecurityModalProps) {
    const { state, dispatch, backend } = useLumo();
    const [pin, setPin] = useState("");
    const [error, setError] = useState("");
    const [verified, setVerified] = useState(false);
    const [copied, setCopied] = useState(false);
    const [loading, setLoading] = useState(false);
    const [invitation, setInvitation] = useState<InvitationData | null>(null);
    const [inviteRole, setInviteRole] = useState<"controller" | "controlled">("controlled");

    useEffect(() => {
        if (action) return;
        setPin("");
        setError("");
        setVerified(false);
        setCopied(false);
        setLoading(false);
        setInvitation(null);
        setInviteRole("controlled");
    }, [action]);

    const close = () => {
        onClose();
    };

    const verify = async (event: FormEvent) => {
        event.preventDefault();
        if (!/^\d{6}$/.test(pin)) {
            setError("Introduce las 6 cifras del PIN");
            return;
        }
        try {
            setLoading(true);
            await backend.verifyPin(pin);
            if (action === "leave") {
                await backend.leaveGroup(pin);
                close();
                window.setTimeout(() => dispatch({ type: "LEAVE_GROUP" }), 220);
                return;
            }

            const created = await backend.createInvitation(pin, inviteRole);
            const invite =
                created ??
                ({
                    invitationId: "preview",
                    token: "preview-invitation",
                    groupName: state.group.name,
                    groupCode: state.group.code,
                    expiresAtMs: Date.now() + 10 * 60_000,
                    role: inviteRole,
                } satisfies InvitationData);
            setInvitation(invite);
            if (!backend.isNative()) {
                try {
                    sessionStorage.setItem(
                        "lumo.preview-invite",
                        JSON.stringify({
                            version: 2,
                            kind: "lumo-group-invite",
                            invitationId: invite.invitationId,
                            token: invite.token,
                            expiresAt: invite.expiresAtMs,
                            apiOrigin: backend.apiOrigin(),
                            role: invite.role,
                        }),
                    );
                } catch {
                    // Session-only browser preview; the native app never stores QR tokens here.
                }
            }
            setVerified(true);
            setError("");
        } catch (requestError) {
            setError(
                requestError instanceof Error
                    ? requestError.message
                    : "No se ha podido completar la acción",
            );
        } finally {
            setLoading(false);
        }
    };

    const copyPin = async () => {
        try {
            await navigator.clipboard.writeText(pin);
            setCopied(true);
            setError("");
        } catch {
            setError("No se ha podido copiar. Puedes compartir el PIN que aparece en pantalla.");
        }
    };

    const isInvite = action === "invite";

    return (
        <Modal
            open={Boolean(action)}
            onClose={close}
            eyebrow="Protegido por PIN"
            title={isInvite ? "Invitar a un miembro" : "Salir del grupo"}
        >
            {isInvite && verified ? (
                <div css={css({ display: "grid", gap: 16 })}>
                    <Pill tone="green">
                        <FiCheck /> PIN verificado
                    </Pill>
                    <div
                        css={css({
                            display: "grid",
                            justifyItems: "center",
                            gap: 15,
                            padding: "20px 16px 18px",
                            border: "1px solid var(--lumo-border)",
                            borderRadius: 24,
                            background:
                                "radial-gradient(circle at 50% 0%, rgba(165,131,225,.2), transparent 58%), var(--lumo-bg)",
                        })}
                    >
                        <div
                            css={css({
                                position: "relative",
                                width: "min(100%, 246px)",
                                display: "grid",
                                placeItems: "center",
                                padding: 12,
                                border: "1px solid rgba(104,66,166,.14)",
                                borderRadius: 24,
                                background: "#fff",
                                boxShadow:
                                    "0 16px 38px rgba(47,38,57,.1), 0 2px 8px rgba(104,66,166,.08)",
                                "&::before": {
                                    content: '""',
                                    position: "absolute",
                                    inset: 5,
                                    border: "1px solid rgba(165,131,225,.13)",
                                    borderRadius: 19,
                                    pointerEvents: "none",
                                },
                            })}
                        >
                            <QRCodeSVG
                                value={JSON.stringify({
                                    version: 2,
                                    kind: "lumo-group-invite",
                                    invitationId: invitation?.invitationId,
                                    token: invitation?.token,
                                    expiresAt: invitation?.expiresAtMs,
                                    apiOrigin: backend.apiOrigin(),
                                    role: invitation?.role,
                                })}
                                size={220}
                                level="H"
                                boostLevel
                                bgColor="#ffffff"
                                fgColor="#2b2630"
                                marginSize={4}
                                imageSettings={{
                                    src: lumoLogo,
                                    width: 38,
                                    height: 38,
                                    excavate: true,
                                }}
                                title={`Invitación al grupo ${state.group.name}`}
                                css={css({
                                    position: "relative",
                                    zIndex: 1,
                                    width: "100%",
                                    height: "auto",
                                    aspectRatio: "1",
                                })}
                            />
                        </div>
                        <div css={css({ display: "grid", justifyItems: "center", gap: 3 })}>
                            <strong css={css({ fontSize: 16 })}>{state.group.name}</strong>
                            <span
                                css={css({
                                    color: "var(--lumo-text-muted)",
                                    fontSize: 10,
                                    letterSpacing: ".08em",
                                })}
                            >
                                {state.group.code}
                            </span>
                            <span
                                css={css({
                                    marginTop: 3,
                                    color: "var(--lumo-text-secondary)",
                                    fontSize: 10,
                                })}
                            >
                                Invitación segura de Lumo
                            </span>
                            <Pill tone={invitation?.role === "controller" ? "purple" : "green"}>
                                {invitation?.role === "controller" ? <FiShield /> : <FiMapPin />}
                                {invitation?.role === "controller"
                                    ? "Nuevo controlador"
                                    : "Teléfono controlado"}
                            </Pill>
                        </div>
                    </div>
                    <p
                        css={css({
                            color: "var(--lumo-text-secondary)",
                            fontSize: 12,
                            lineHeight: 1.5,
                            textAlign: "center",
                        })}
                    >
                        Escanea el QR desde el otro móvil y después introduce este PIN:
                    </p>
                    <div
                        css={css({
                            padding: "13px 16px",
                            border: "1px solid var(--lumo-border-strong)",
                            borderRadius: 17,
                            color: "var(--lumo-primary)",
                            background: "var(--lumo-lavender)",
                            fontSize: 22,
                            fontWeight: 700,
                            letterSpacing: ".2em",
                            textAlign: "center",
                        })}
                    >
                        {pin}
                    </div>
                    <Button fullWidth icon={copied ? FiCheck : FiCopy} onClick={copyPin}>
                        {copied ? "PIN copiado" : "Copiar PIN"}
                    </Button>
                    {error && (
                        <p role="alert" css={css({ color: "var(--lumo-danger)", fontSize: 12 })}>
                            {error}
                        </p>
                    )}
                </div>
            ) : (
                <form onSubmit={verify} css={css({ display: "grid", gap: 16 })}>
                    <div
                        css={css({
                            display: "flex",
                            alignItems: "flex-start",
                            gap: 10,
                            color: "var(--lumo-text-secondary)",
                            fontSize: 12,
                            lineHeight: 1.5,
                        })}
                    >
                        {isInvite ? (
                            <FiUserPlus size={18} css={css({ flex: "0 0 auto", marginTop: 1 })} />
                        ) : (
                            <FiKey size={18} css={css({ flex: "0 0 auto", marginTop: 1 })} />
                        )}
                        {isInvite
                            ? "Introduce el PIN para ver los datos que deberá usar el nuevo miembro."
                            : "Introduce el PIN para confirmar que quieres desvincular este teléfono."}
                    </div>
                    {isInvite && (
                        <fieldset
                            css={css({
                                minWidth: 0,
                                display: "grid",
                                gridTemplateColumns: "1fr 1fr",
                                gap: 8,
                                padding: 0,
                                border: 0,
                            })}
                        >
                            <legend
                                css={css({
                                    gridColumn: "1 / -1",
                                    marginBottom: 8,
                                    color: "var(--lumo-text-muted)",
                                    fontSize: 10,
                                    letterSpacing: ".06em",
                                    textTransform: "uppercase",
                                })}
                            >
                                Rol del nuevo teléfono
                            </legend>
                            {(
                                [
                                    {
                                        role: "controlled" as const,
                                        icon: FiMapPin,
                                        title: "Controlado",
                                        detail: "Comparte ubicación",
                                    },
                                    {
                                        role: "controller" as const,
                                        icon: FiShield,
                                        title: "Controlador",
                                        detail: "Recibe avisos",
                                    },
                                ] as const
                            ).map((option) => (
                                <button
                                    key={option.role}
                                    type="button"
                                    aria-pressed={inviteRole === option.role}
                                    onClick={() => setInviteRole(option.role)}
                                    css={css({
                                        minHeight: 76,
                                        display: "grid",
                                        justifyItems: "start",
                                        gap: 4,
                                        padding: 12,
                                        border:
                                            inviteRole === option.role
                                                ? "1.5px solid var(--lumo-primary)"
                                                : "1px solid var(--lumo-border)",
                                        borderRadius: 16,
                                        color: "var(--lumo-text)",
                                        background:
                                            inviteRole === option.role
                                                ? "var(--lumo-lavender)"
                                                : "#fff",
                                        textAlign: "left",
                                    })}
                                >
                                    <option.icon color="var(--lumo-primary)" size={17} />
                                    <strong css={css({ fontSize: 12 })}>{option.title}</strong>
                                    <span
                                        css={css({
                                            color: "var(--lumo-text-secondary)",
                                            fontSize: 9,
                                        })}
                                    >
                                        {option.detail}
                                    </span>
                                </button>
                            ))}
                        </fieldset>
                    )}
                    <Field
                        type="password"
                        inputMode="numeric"
                        autoComplete="off"
                        autoFocus
                        label="PIN del grupo"
                        placeholder="6 cifras"
                        icon={FiLock}
                        maxLength={6}
                        value={pin}
                        error={error}
                        onChange={(event) => {
                            setPin(event.target.value.replace(/\D/g, "").slice(0, 6));
                            setError("");
                        }}
                    />
                    <Button
                        type="submit"
                        fullWidth
                        disabled={pin.length !== 6}
                        variant={isInvite ? "primary" : "danger"}
                        icon={isInvite ? FiUserPlus : FiLogOut}
                        loading={loading}
                    >
                        {isInvite ? "Ver invitación" : "Salir del grupo"}
                    </Button>
                </form>
            )}
        </Modal>
    );
}
