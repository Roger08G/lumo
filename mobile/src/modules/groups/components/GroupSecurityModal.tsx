import { useEffect, useState, type FormEvent } from "react";
import { css } from "@emotion/react";
import { FiCheck, FiCopy, FiKey, FiLock, FiLogOut, FiUserPlus } from "react-icons/fi";
import { QRCodeSVG } from "qrcode.react";

import { useLumo } from "@app/state/lumoContext.ts";
import { Button, Field, Modal, Pill } from "@shared/components/ui.tsx";
import type { InvitationData } from "@shared/services/lumoBackend.ts";

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

    useEffect(() => {
        if (action) return;
        setPin("");
        setError("");
        setVerified(false);
        setCopied(false);
        setLoading(false);
        setInvitation(null);
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

            const created = await backend.createInvitation(pin);
            const invite =
                created ??
                ({
                    invitationId: "preview",
                    token: "preview-invitation",
                    groupName: state.group.name,
                    groupCode: state.group.code,
                    expiresAtMs: Date.now() + 15 * 60_000,
                } satisfies InvitationData);
            setInvitation(invite);
            try {
                localStorage.setItem(
                    "lumo.preview-invite",
                    JSON.stringify({
                        version: 1,
                        kind: "lumo-group-invite",
                        name: state.group.name,
                        code: state.group.code,
                        supervisorName: state.group.supervisorName,
                        trackedPersonName: state.group.trackedPersonName,
                        token: invite.token,
                    }),
                );
            } catch {
                // The real QR is still usable when browser preview storage is unavailable.
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
                            gap: 13,
                            padding: "18px 16px",
                            border: "1px solid var(--lumo-border)",
                            borderRadius: 22,
                            background: "var(--lumo-bg)",
                        })}
                    >
                        <div
                            css={css({
                                display: "grid",
                                placeItems: "center",
                                padding: 14,
                                border: "1px solid var(--lumo-border)",
                                borderRadius: 18,
                                background: "#fff",
                                boxShadow: "0 8px 24px rgba(47,38,57,.06)",
                            })}
                        >
                            <QRCodeSVG
                                value={JSON.stringify({
                                    version: 1,
                                    kind: "lumo-group-invite",
                                    name: state.group.name,
                                    code: state.group.code,
                                    supervisorName: state.group.supervisorName,
                                    trackedPersonName: state.group.trackedPersonName,
                                    token: invitation?.token,
                                })}
                                size={166}
                                level="M"
                                bgColor="#ffffff"
                                fgColor="#2b2630"
                                marginSize={1}
                                title={`Invitación al grupo ${state.group.name}`}
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
