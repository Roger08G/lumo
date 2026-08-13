import type { PlaceTone } from "@shared/types/lumo.ts";

export const PLACE_TONES: PlaceTone[] = ["yellow", "green", "blue", "pink", "purple"];

export const PLACE_PALETTE: Record<PlaceTone, { background: string; foreground: string }> = {
    yellow: { background: "#faedba", foreground: "#89671a" },
    green: { background: "#dcefe3", foreground: "#34735a" },
    blue: { background: "#dceaf7", foreground: "#3f6f99" },
    pink: { background: "#f6dee6", foreground: "#a84e69" },
    purple: { background: "#eae1f7", foreground: "#6842a6" },
};

export function randomPlaceTone(avoid?: PlaceTone): PlaceTone {
    const choices = avoid ? PLACE_TONES.filter((tone) => tone !== avoid) : PLACE_TONES;
    return choices[Math.floor(Math.random() * choices.length)] ?? "purple";
}
