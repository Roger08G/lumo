import { createContext, useContext, type Dispatch } from "react";

import type { LumoAction, LumoState } from "@shared/types/lumo.ts";

export interface LumoContextValue {
    state: LumoState;
    dispatch: Dispatch<LumoAction>;
}

export const LumoContext = createContext<LumoContextValue | null>(null);

export function useLumo() {
    const context = useContext(LumoContext);
    if (!context) throw new Error("useLumo must be used inside LumoProvider");
    return context;
}
