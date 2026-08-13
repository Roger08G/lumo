import { useEffect, useState, type FormEvent } from "react";
import { css } from "@emotion/react";
import { FiLock } from "react-icons/fi";
import type { IconType } from "react-icons";

import { useLumo } from "@app/state/lumoContext.ts";
import { Button, Field, Modal, type ButtonVariant } from "@shared/components/ui.tsx";

interface ProtectedActionModalProps {
    open: boolean;
    title: string;
    description: string;
    confirmLabel: string;
    icon: IconType;
    variant?: ButtonVariant;
    onClose: () => void;
    onConfirm: (pin: string) => Promise<void> | void;
}

export function ProtectedActionModal({
    open,
    title,
    description,
    confirmLabel,
    icon,
    variant = "primary",
    onClose,
    onConfirm,
}: ProtectedActionModalProps) {
    const { backend } = useLumo();
    const [pin, setPin] = useState("");
    const [error, setError] = useState("");
    const [loading, setLoading] = useState(false);

    useEffect(() => {
        if (open) return;
        setPin("");
        setError("");
        setLoading(false);
    }, [open]);

    const submit = async (event: FormEvent) => {
        event.preventDefault();
        setLoading(true);
        try {
            await backend.verifyPin(pin);
            await onConfirm(pin);
            onClose();
        } catch (requestError) {
            setError(
                requestError instanceof Error ? requestError.message : "El PIN no es correcto",
            );
            setLoading(false);
        }
    };

    return (
        <Modal open={open} onClose={onClose} eyebrow="Acción protegida" title={title}>
            <form onSubmit={submit} css={css({ display: "grid", gap: 16 })}>
                <p
                    css={css({
                        color: "var(--lumo-text-secondary)",
                        fontSize: 12,
                        lineHeight: 1.55,
                    })}
                >
                    {description}
                </p>
                <Field
                    autoFocus
                    type="password"
                    inputMode="numeric"
                    autoComplete="off"
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
                    icon={icon}
                    variant={variant}
                    disabled={pin.length !== 6}
                    loading={loading}
                >
                    {confirmLabel}
                </Button>
            </form>
        </Modal>
    );
}
