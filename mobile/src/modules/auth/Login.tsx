import React from "react";

import { AuthButtons } from "@modules/auth/components/AuthButtons.tsx";

const Auth: React.FC = () => {
    return (
        <main style={{ gap: "2.5rem" }}>
            <h1>¡Bienvenido a Lumo!</h1>
            <AuthButtons />
        </main>
    );
};

export default Auth;
